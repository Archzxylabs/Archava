use super::*;

pub async fn workspace(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    a: &OrgAccess,
    company: Option<Uuid>,
    page: Page<'_>,
) -> Result<Value, PurchaseError> {
    let mut result = json!({"domain":"accounting","companyId":company,"permissions":a.permissions,"books":[],"accounts":[],"periods":[],"journals":[],"selected":null,"detail":null,"currencies":accounting::currencies(),"collections":[{"id":"books","label":"Official book","hasMore":false},{"id":"accounts","label":"Accounts","hasMore":false},{"id":"periods","label":"Financial periods","hasMore":false},{"id":"journals","label":"Journals","hasMore":false}],"requiresCompany":company.is_none(),"pageLimit":page.limit,"notice":"Choose a managed company and explicitly configure its book. Amounts use exact ISO minor units; provider payment units require a separate mapping. External books need a tested connector before synchronization is ready."});
    let Some(company) = company else {
        return Ok(result);
    };
    let company = company_trade::resolve(conn, tenant, p, company).await?;
    result["companyActive"] = json!(company.active);
    if !company.active {
        result["notice"]=json!("This company is inactive for new business. Its existing book remains available for authorized financial corrections, posting and period close. Reactivate the company before setting up a new book.");
    }
    let books:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'companyId',company_id,'companyRevision',company_revision,'companyCode',company_code,'companyName',company_name,'code',code,'name',name,'currency',currency,'minorUnits',minor_units,'currencyCatalogue',currency_catalogue,'sourceAuthority',source_authority,'externalSystem',external_system,'independentPosting',independent_posting,'revision',revision,'createdAt',created_at,'externalConnectorReady',false) FROM archava_accounting.books WHERE tenant_id=$1 AND company_id=$2 AND ($3 OR owner_id=$4) ORDER BY code LIMIT 2").bind(tenant).bind(company.id).bind(a.scope.covers_all()).bind(&a.owner_id).fetch_all(&mut *conn).await.map_err(db)?;
    let Some(key) = books.first().and_then(|b| b["id"].as_str()) else {
        return Ok(result);
    };
    let key = id(key)?;
    book(conn, tenant, p, a, key, false).await?;
    result["configuredBook"] = books[0].clone();
    result["books"] = json!(books);
    result["invoicePolicy"] = sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'revision',revision,'sourceUnit',source_unit,'currencyCatalogue',currency_catalogue,'mappings',mappings) FROM archava_accounting.invoice_policies WHERE tenant_id=$1 AND book_id=$2")
        .bind(tenant).bind(key).fetch_optional(&mut *conn).await.map_err(db)?.unwrap_or(Value::Null);
    // The unit a source bridge reads on this book: whole Rupiah on an IDR book,
    // the currency's ISO minor unit on any other.
    let (unit_name, unit_detail) = match books[0]["currency"].as_str() {
        Some("IDR") | None => ("whole-IDR".to_string(), String::new()),
        Some(code) => (
            format!("{code} minor-unit"),
            format!(
                " Source amounts are {code} minor units ({} decimal places) and post unchanged.",
                books[0]["minorUnits"].as_i64().unwrap_or(0)
            ),
        ),
    };
    result["invoicePostingNotice"] = json!(format!("The optional invoice policy captures only future company invoices under an explicit {unit_name} unit contract.{unit_detail} Existing issued invoices require separate opening/import review. Verified collections go to payment clearing, not bank cash. An active policy keeps Billing and Accounting enabled."));
    result["purchasePolicy"]=sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'revision',revision,'sourceUnit',source_unit,'currencyCatalogue',currency_catalogue,'mappings',mappings) FROM archava_accounting.purchase_policies WHERE tenant_id=$1 AND book_id=$2").bind(tenant).bind(key).fetch_optional(&mut *conn).await.map_err(db)?.unwrap_or(Value::Null);
    result["purchasePostingNotice"]=json!(format!("An explicit {unit_name} purchase policy captures only orders confirmed after setup.{unit_detail} Accepted goods and landed fees become inventory/GRNI and separate fee accrual; matched goods claims become supplier payable sources. Review and posting remain separate. Old confirmations need opening/import review. Supplier payment, bank reconciliation and aging are separate requirements. Active capture keeps Purchasing, Commerce and Accounting enabled."));
    result["stockPolicy"]=sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'revision',revision,'sourceUnit',source_unit,'currencyCatalogue',currency_catalogue,'cogsAccount',jsonb_build_object('accountId',cogs_account_id,'expectedAccountRevision',account_revision)) FROM archava_accounting.stock_policies WHERE tenant_id=$1 AND book_id=$2").bind(tenant).bind(key).fetch_optional(&mut *conn).await.map_err(db)?.unwrap_or(Value::Null);
    result["stockPostingNotice"]=json!("The stock cost policy captures future fulfillment and returns from reviewed original acquisition costs. Restocking reverses the original recognized COGS account, even after policy changes; discard adds no inventory. Unknown or unrecognized historical costs require opening/import review. Draft and independent posting remain separate.");
    if a.holds(&["payables.read"]) {
        result["payablesSummary"] = payables::summary(conn, tenant, key).await?;
        result["payablesNotice"] = json!("Supplier payments record actual imported bank movements. Drafts reserve the bill and bank amount; separate posting recognizes payment. Only posted bank returns restore the payable. Terms, legacy opening balances and full subledger close require separate review.");
    }
    page::load(conn, tenant, key, page, a, &mut result).await?;
    Ok(result)
}

fn exact(v: &Value, key: &str) -> Result<i128, PurchaseError> {
    v[key]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ErpError::Conflict.into())
}
pub async fn reports(
    conn: &mut PgConnection,
    tenant: &str,
    p: &PurchasePrincipal,
    a: &OrgAccess,
    key: Uuid,
    start: &str,
    end: &str,
) -> Result<Value, PurchaseError> {
    require(a, "reports.read")?;
    let b = book(conn, tenant, p, a, key, false).await?;
    if b.source != "native" {
        return Err(ErpError::Conflict.into());
    }
    let header:Value=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'companyId',company_id,'currency',currency,'minorUnits',minor_units,'code',code,'name',name) FROM archava_accounting.books WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(key).fetch_one(&mut *conn).await.map_err(db)?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',a.id,'code',a.code,'name',a.name,'class',a.class,'cashRole',a.cash_role,'openingNetMinor',coalesce(sum(l.debit_minor::numeric-l.credit_minor::numeric) FILTER(WHERE j.entry_date<$3::text::date),0)::text,'debitMinor',coalesce(sum(l.debit_minor::numeric) FILTER(WHERE j.entry_date>=$3::text::date),0)::text,'creditMinor',coalesce(sum(l.credit_minor::numeric) FILTER(WHERE j.entry_date>=$3::text::date),0)::text,'closingNetMinor',coalesce(sum(l.debit_minor::numeric-l.credit_minor::numeric),0)::text) FROM archava_accounting.accounts a LEFT JOIN (archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id AND j.state='posted' AND j.entry_date<=$4::text::date) ON l.tenant_id=a.tenant_id AND l.account_id=a.id WHERE a.tenant_id=$1 AND a.book_id=$2 GROUP BY a.tenant_id,a.id ORDER BY a.code")
        .bind(tenant).bind(key).bind(start).bind(end).fetch_all(&mut *conn).await.map_err(db)?;
    let (
        mut debits,
        mut credits,
        mut assets,
        mut liabilities,
        mut equity,
        mut revenue,
        mut costs,
        mut expenses,
        mut cumulative_earnings,
    ) = (
        0_i128, 0_i128, 0_i128, 0_i128, 0_i128, 0_i128, 0_i128, 0_i128, 0_i128,
    );
    for row in &rows {
        let d = exact(row, "debitMinor")?;
        let c = exact(row, "creditMinor")?;
        let n = exact(row, "closingNetMinor")?;
        debits = debits.checked_add(d).ok_or(ErpError::Conflict)?;
        credits = credits.checked_add(c).ok_or(ErpError::Conflict)?;
        let add = |v: &mut i128, n: i128| -> Result<(), PurchaseError> {
            *v = v.checked_add(n).ok_or(ErpError::Conflict)?;
            Ok(())
        };
        match row["class"].as_str().ok_or(ErpError::Conflict)? {
            "asset" => add(&mut assets, n)?,
            "liability" => add(&mut liabilities, -n)?,
            "equity" => add(&mut equity, -n)?,
            "revenue" => {
                add(&mut revenue, c - d)?;
                add(&mut cumulative_earnings, -n)?;
            }
            "cost_of_goods_sold" => {
                add(&mut costs, d - c)?;
                add(&mut cumulative_earnings, -n)?;
            }
            "expense" => {
                add(&mut expenses, d - c)?;
                add(&mut cumulative_earnings, -n)?;
            }
            _ => return Err(ErpError::Conflict.into()),
        }
    }
    let cash:Value=sqlx::query_scalar("WITH flows AS (SELECT j.id,sum(CASE WHEN l.cash_role='cash' THEN l.debit_minor::numeric-l.credit_minor::numeric ELSE 0 END) amount,count(DISTINCT l.cash_role) FILTER(WHERE l.cash_role<>'cash') categories,min(l.cash_role) FILTER(WHERE l.cash_role<>'cash') category FROM archava_accounting.journals j JOIN archava_accounting.journal_lines l ON l.tenant_id=j.tenant_id AND l.journal_id=j.id WHERE j.tenant_id=$1 AND j.book_id=$2 AND j.state='posted' AND j.entry_date BETWEEN $3::text::date AND $4::text::date GROUP BY j.id), grouped AS(SELECT amount,CASE WHEN EXISTS(SELECT 1 FROM archava_accounting.clearing_settlements cs WHERE cs.tenant_id=$1 AND cs.id=flows.id) THEN 'operating' WHEN categories=1 AND category IN ('operating','investing','financing') THEN category ELSE 'unclassified' END category FROM flows WHERE amount<>0) SELECT jsonb_build_object('operatingMinor',coalesce(sum(amount) FILTER(WHERE category='operating'),0)::text,'investingMinor',coalesce(sum(amount) FILTER(WHERE category='investing'),0)::text,'financingMinor',coalesce(sum(amount) FILTER(WHERE category='financing'),0)::text,'unclassifiedMinor',coalesce(sum(amount) FILTER(WHERE category='unclassified'),0)::text,'unclassifiedJournals',count(*) FILTER(WHERE category='unclassified'),'netChangeMinor',coalesce(sum(amount),0)::text) FROM grouped")
        .bind(tenant).bind(key).bind(start).bind(end).fetch_one(&mut *conn).await.map_err(db)?;
    let activity_count:i64=sqlx::query_scalar("SELECT count(*) FROM archava_accounting.journals WHERE tenant_id=$1 AND book_id=$2 AND state='posted' AND entry_date BETWEEN $3::text::date AND $4::text::date").bind(tenant).bind(key).bind(start).bind(end).fetch_one(&mut *conn).await.map_err(db)?;
    let source_coverage:Value=sqlx::query_scalar("WITH sources AS(SELECT s.source_date,s.amount_minor<>0 AS monetary,EXISTS(SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted') AS posted FROM archava_accounting.invoice_sources s WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.source_date<=$3::text::date UNION ALL SELECT s.source_date,s.goods_minor<>0 OR s.landed_fee_minor<>0,EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted') FROM archava_accounting.purchase_sources s WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.source_date<=$3::text::date UNION ALL SELECT s.source_date,s.amount_minor<>0,EXISTS(SELECT 1 FROM archava_accounting.stock_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted') FROM archava_accounting.stock_sources s WHERE s.tenant_id=$1 AND s.book_id=$2 AND s.source_date<=$3::text::date) SELECT jsonb_build_object('capturedCount',count(*),'postedCount',count(*) FILTER(WHERE posted),'noMonetaryEffectCount',count(*) FILTER(WHERE NOT monetary),'unpostedCount',count(*) FILTER(WHERE monetary AND NOT posted),'through',$3::text,'allCapturedSourcesThroughEnd',true,'sourceModules',jsonb_build_array('billing','purchasing','commerce'),'legacyInvoicesImported',false,'legacyPurchasesImported',false,'legacyStockImported',false,'bankReconciliationIncluded',false,'fullSubledgerClose',false) FROM sources")
        .bind(tenant).bind(key).bind(end).fetch_one(conn).await.map_err(db)?;
    let profit = revenue
        .checked_sub(costs)
        .and_then(|v| v.checked_sub(expenses))
        .ok_or(ErpError::Conflict)?;
    let total_equity = equity
        .checked_add(cumulative_earnings)
        .ok_or(ErpError::Conflict)?;
    let rhs = liabilities
        .checked_add(total_equity)
        .ok_or(ErpError::Conflict)?;
    Ok(
        json!({"sourceCoverage":source_coverage,"book":header,"from":start,"to":end,"observedAt":null,"coverage":{"allPostedJournalsInFilter":true,"postedJournalCount":activity_count,"draftsIncluded":false,"voidsIncluded":false},"trialBalance":{"accounts":rows,"debitMinor":debits.to_string(),"creditMinor":credits.to_string(),"balanced":debits==credits},"profitAndLoss":{"revenueMinor":revenue.to_string(),"costOfGoodsSoldMinor":costs.to_string(),"expenseMinor":expenses.to_string(),"netProfitMinor":profit.to_string()},"balanceSheet":{"asOf":end,"assetsMinor":assets.to_string(),"liabilitiesMinor":liabilities.to_string(),"contributedAndPostedEquityMinor":equity.to_string(),"unclosedCumulativeEarningsMinor":cumulative_earnings.to_string(),"totalEquityMinor":total_equity.to_string(),"balanced":assets==rhs},"cashFlow":cash,"notice":"Exact functional-currency minor units. Cash flow uses reviewed counterpart categories and structured verified invoice-to-bank settlement sources; mixed or missing classifications are explicit exceptions. Tax, statutory filing, bank reconciliation and operational subledger control are separate requirements."}),
    )
}
