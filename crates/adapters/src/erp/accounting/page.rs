use super::*;
#[derive(Clone, Copy)]
pub struct Page<'a> {
    pub search: &'a str,
    pub collection: Option<&'a str>,
    pub selected: Option<Uuid>,
    pub cursor: Option<Uuid>,
    pub limit: i64,
    /// The date the `tax_codes` collection answers "which version applies" for.
    pub as_of: Option<&'a str>,
}
impl Page<'_> {
    pub fn valid(&self) -> bool {
        self.search.chars().count() <= 160
            && (1..=100).contains(&self.limit)
            && self.collection.is_none_or(|c| {
                matches!(
                    c,
                    "books"
                        | "accounts"
                        | "periods"
                        | "journals"
                        | "invoice_policies"
                        | "invoice_sources"
                        | "purchase_policies"
                        | "purchase_sources"
                        | "stock_policies"
                        | "stock_sources"
                        | "bank_accounts"
                        | "bank_statements"
                        | "bank_rows"
                        | "bank_allocations"
                        | "bank_unmatches"
                        | "bank_policies"
                        | "bank_reconciliations"
                        | "payables"
                        | "payable_settlements"
                        | "clearing_sources"
                        | "clearing_settlements"
                        | "invoice_term_sources"
                        | "supplier_term_sources"
                        | "document_terms"
                        | "tax_codes"
                )
            })
            && (self.selected.is_none() || self.cursor.is_none())
            && self.as_of.is_none_or(|date| {
                archava_domain::erp::accommodation::parse_calendar_date(date).is_some()
            })
    }
}

pub(super) async fn load(
    conn: &mut PgConnection,
    tenant: &str,
    key: Uuid,
    page: Page<'_>,
    access: &OrgAccess,
    result: &mut Value,
) -> Result<(), PurchaseError> {
    let terms_read = access.holds(&["terms.read"]);
    let bank_read = access.holds(&["bank.read"]);
    let payables_read = access.holds(&["payables.read"]);
    let clearing_read = access.holds(&["clearing.read"]);
    let tax_read = access.holds(&["tax.read"]);
    let mut metadata = Vec::new();
    for (table,label,projection,search) in [
        ("tax_codes","Tax codes entered by the company",tax::projection(),"s.code||' '||s.kind"),
        ("invoice_term_sources","Original customer invoice due terms",terms::invoice_projection(),"s.invoice_code"),
        ("supplier_term_sources","Original supplier bill due terms",terms::supplier_projection(),"s.document_id::text||' '||s.order_id::text"),
        ("document_terms","Reviewed due terms history",terms::history_projection(),"s.document_id::text||' '||s.document_kind||' '||s.reason"),
        ("clearing_sources","Collected invoice money awaiting bank settlement",clearing::source_projection(),"invoice_code||' '||effect"),
        ("clearing_settlements","Bank settlements of collected invoice money",clearing::settlement_projection(),"source_id::text||' '||effect||' '||(SELECT j.code FROM archava_accounting.journals j WHERE j.tenant_id=s.tenant_id AND j.id=s.id)"),
        ("payables","Supplier bill balances",payables::bill_projection(),"document_id::text||' '||order_id::text"),
        ("payable_settlements","Supplier bank payments and returns",payables::settlement_projection(),"source_id::text||' '||effect||' '||(SELECT j.code FROM archava_accounting.journals j WHERE j.tenant_id=s.tenant_id AND j.id=s.id)"),
        ("bank_accounts","Bank accounts","jsonb_build_object('id',s.id,'bookId',s.book_id,'code',s.code,'name',s.name,'accountId',s.account_id,'bindingAccountRevision',s.account_revision,'accountRevision',(SELECT a.revision FROM archava_accounting.accounts a WHERE a.tenant_id=s.tenant_id AND a.id=s.account_id),'accountActive',(SELECT a.active FROM archava_accounting.accounts a WHERE a.tenant_id=s.tenant_id AND a.id=s.account_id),'bankPolicyId',(SELECT p.id FROM archava_accounting.bank_policies p WHERE p.tenant_id=s.tenant_id AND p.bank_account_id=s.id),'bankPolicyRevision',coalesce((SELECT p.revision FROM archava_accounting.bank_policies p WHERE p.tenant_id=s.tenant_id AND p.bank_account_id=s.id),0),'revision',s.revision)","code||' '||name"),
        ("bank_policies","Bank reconciliation basis","jsonb_build_object('id',s.id,'bookId',s.book_id,'bankAccountId',s.bank_account_id,'code',(SELECT b.code FROM archava_accounting.bank_accounts b WHERE b.tenant_id=s.tenant_id AND b.id=s.bank_account_id),'fromDate',s.from_date,'independentReview',s.independent_review,'revision',s.revision,'sealed',EXISTS(SELECT 1 FROM archava_accounting.bank_reconciliations x WHERE x.tenant_id=s.tenant_id AND x.bank_policy_id=s.id))","bank_account_id::text"),
        ("bank_reconciliations","Reviewed bank periods","jsonb_build_object('id',s.id,'bookId',s.book_id,'bankPolicyId',s.bank_policy_id,'periodId',s.period_id,'code','Bank period review','reviewedPeriodRevision',s.reviewed_period_revision,'policyRevision',s.policy_revision,'check',s.check_snapshot,'reviewedBy',s.created_by,'createdAt',s.created_at,'revision',s.revision)","period_id::text||' '||created_by"),
        ("bank_statements","Imported bank statements","jsonb_build_object('id',s.id,'bookId',s.book_id,'bankAccountId',s.bank_account_id,'code',s.reference,'fromDate',s.from_date,'toDate',s.to_date,'currency',s.currency,'openingMinor',s.opening_minor::text,'closingMinor',s.closing_minor::text,'rowCount',s.row_count,'revision',s.revision,'createdBy',s.created_by,'evidenceOnly',true)","reference"),
        ("bank_rows","Statement transactions","jsonb_build_object('id',s.id,'bookId',s.book_id,'bankAccountId',s.bank_account_id,'statementId',s.statement_id,'code',s.external_ref,'externalRef',s.external_ref,'ordinal',s.ordinal,'date',s.entry_date,'amountMinor',s.amount_minor::text,'remainingMinor',archava_accounting.bank_row_remaining(s.tenant_id,s.id)::text,'description',s.description,'revision',1)","external_ref||' '||description"),
        ("bank_allocations","Cash matches","jsonb_build_object('id',s.id,'bookId',s.book_id,'bankRowId',s.bank_row_id,'journalId',s.journal_id,'journalLineOrdinal',s.journal_line_ordinal,'amountMinor',s.amount_minor::text,'revision',s.revision,'code','Cash match · '||s.journal_line_ordinal::text,'state',CASE WHEN EXISTS(SELECT 1 FROM archava_accounting.bank_unmatches x WHERE x.tenant_id=s.tenant_id AND x.allocation_id=s.id) THEN 'reversed' ELSE 'matched' END)","journal_id::text||' '||bank_row_id::text"),
        ("bank_unmatches","Cash match corrections","jsonb_build_object('id',s.id,'bookId',s.book_id,'allocationId',s.allocation_id,'revision',s.revision,'code','Reversed cash match','createdBy',s.created_by,'createdAt',s.created_at)","allocation_id::text"),
        ("stock_policies","Stock cost posting policies","jsonb_build_object('id',id,'bookId',book_id,'code','Stock cost posting policy','sourceUnit',source_unit,'currencyCatalogue',currency_catalogue,'cogsAccount',jsonb_build_object('accountId',cogs_account_id,'expectedAccountRevision',account_revision),'revision',revision)","source_unit"),
        ("stock_sources","Captured original stock costs",stock_projection(),"order_id::text||' '||effect"),
        ("purchase_policies","Purchase posting policies","jsonb_build_object('id',id,'bookId',book_id,'code','Purchase posting policy','sourceUnit',source_unit,'currencyCatalogue',currency_catalogue,'mappings',mappings,'revision',revision)","source_unit"),
        ("purchase_sources","Captured purchase sources","jsonb_build_object('id',s.id,'bookId',s.book_id,'code',s.effect||' · '||s.order_id::text,'orderId',s.order_id,'documentId',s.document_id,'sourceRevision',s.source_revision,'effect',s.effect,'goodsMinor',s.goods_minor::text,'landedFeeMinor',s.landed_fee_minor::text,'sourceDate',s.source_date,'policyId',(SELECT policy_id FROM archava_accounting.purchase_origins o WHERE o.tenant_id=s.tenant_id AND o.id=s.origin_id),'policyRevision',(SELECT policy_revision FROM archava_accounting.purchase_origins o WHERE o.tenant_id=s.tenant_id AND o.id=s.origin_id),'policy',(SELECT policy_snapshot FROM archava_accounting.purchase_origins o WHERE o.tenant_id=s.tenant_id AND o.id=s.origin_id),'revision',1,'sourceState',CASE WHEN s.goods_minor=0 AND s.landed_fee_minor=0 THEN 'no_monetary_effect' WHEN EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted') THEN 'posted' WHEN EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='draft') THEN 'draft' ELSE 'pending' END)||CASE WHEN s.tax_account_id IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('taxMinor',s.tax_minor::text,'taxAccountId',s.tax_account_id,'taxAccountRevision',s.tax_account_revision) END","order_id::text||' '||coalesce(document_id::text,'')||' '||effect"),
        ("invoice_policies","Invoice posting policies","jsonb_build_object('id',id,'bookId',book_id,'code','Invoice posting policy','sourceUnit',source_unit,'currencyCatalogue',currency_catalogue,'mappings',mappings,'revision',revision)","source_unit"),
        ("invoice_sources","Captured invoice sources","jsonb_build_object('id',id,'bookId',book_id,'code',invoice_code||' · '||effect,'invoiceId',invoice_id,'invoiceCode',invoice_code,'invoiceRevision',invoice_revision,'effect',effect,'amountMinor',amount_minor::text,'sourceDate',source_date,'policyId',policy_id,'policyRevision',policy_revision,'policy',policy_snapshot,'revision',1,'sourceState',CASE WHEN amount_minor=0 THEN 'no_monetary_effect' WHEN EXISTS(SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted') THEN 'posted' WHEN EXISTS(SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='draft') THEN 'draft' ELSE 'pending' END)||CASE WHEN tax_account_id IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('taxMinor',tax_minor::text,'taxAccountId',tax_account_id,'taxAccountRevision',tax_account_revision) END","invoice_code||' '||effect"),
        ("accounts","Accounts","jsonb_build_object('id',id,'bookId',book_id,'code',code,'name',name,'class',class,'cashRole',cash_role,'active',active,'revision',revision)","code||' '||name"),
        ("periods","Financial periods","jsonb_build_object('id',id,'bookId',book_id,'code',code,'startDate',start_date,'endDate',end_date,'state',state,'revision',revision)","code||' '||start_date::text||' '||end_date::text"),
        ("journals","Journals","jsonb_build_object('id',id,'bookId',book_id,'periodId',period_id,'code',code,'date',entry_date,'sourceRef',source_ref,'memo',memo,'state',state,'revision',revision,'createdBy',created_by,'postedBy',posted_by,'postedAt',posted_at,'reversalOf',reversal_of,'cashLegs',coalesce((SELECT jsonb_agg(jsonb_build_object('ordinal',l.ordinal,'accountId',l.account_id,'accountCode',l.account_code,'accountName',l.account_name,'debitMinor',l.debit_minor::text,'creditMinor',l.credit_minor::text) ORDER BY l.ordinal) FROM archava_accounting.journal_lines l WHERE l.tenant_id=s.tenant_id AND l.journal_id=s.id AND l.cash_role='cash'),'[]'::jsonb))","code||' '||source_ref||' '||memo"),
    ] {
        if (matches!(table,"invoice_term_sources"|"supplier_term_sources"|"document_terms") && !terms_read)
            || (table=="supplier_term_sources" && !payables_read) {
            if page.collection == Some(table) {return Err(ErpError::Forbidden.into());}
            continue;
        }
        if (table.starts_with("bank_") && !bank_read)
            || (table.starts_with("payable") && !payables_read)
            || (table == "payable_settlements" && !bank_read) {
            if page.collection == Some(table) {return Err(ErpError::Forbidden.into());}
            continue;
        }
        if table == "tax_codes" && !tax_read {
            if page.collection == Some(table) {
                return Err(ErpError::Forbidden.into());
            }
            continue;
        }
        if (table.starts_with("clearing_") && !clearing_read) || (table == "clearing_settlements" && !bank_read) {
            if page.collection == Some(table) {
                return Err(ErpError::Forbidden.into());
            }
            continue;
        }
        if page.collection.is_some_and(|c|c!=table){metadata.push(json!({"id":table,"label":label,"hasMore":false,"nextCursor":null,"notRequested":true}));continue;}
        // Every identifier/expression comes solely from the fixed tuples above.
        let projection = if table == "payables" {
            format!("{projection} || {}",terms::payable_due_projection(terms_read))
        } else if table == "bank_rows" && (payables_read || clearing_read) {
            format!("{projection} || jsonb_build_object('unreservedMinor',(sign(s.amount_minor)*archava_accounting.bank_unreserved(s.tenant_id,s.id))::text)")
        } else { projection.to_string() };
        let (relation,filter) = match table {
            "payables"|"supplier_term_sources" => ("purchase_sources"," AND effect='bill'"),
            "invoice_term_sources" => ("invoice_sources"," AND effect='issue'"),
            "clearing_sources" => ("invoice_sources"," AND effect IN ('collection','refund') AND amount_minor<>0"),
            "document_terms" if !payables_read => ("document_terms"," AND document_kind='receivable'"),
            _ => (table, ""),
        };
        let q=format!("SELECT {projection} FROM archava_accounting.{relation} s WHERE tenant_id=$1 AND book_id=$2{filter} AND ($3::uuid IS NULL OR id=$3) AND ($4::uuid IS NULL OR id>$4) AND strpos(lower({search}),lower($5))>0 ORDER BY id LIMIT $6");
        let mut query = sqlx::query_scalar(sqlx::AssertSqlSafe(q.as_str())).bind(tenant).bind(key).bind(page.selected).bind(page.cursor).bind(page.search).bind(page.limit+1);
        // Only the tax projection names $7; an unused parameter has no type.
        if table == "tax_codes" {
            query = query.bind(page.as_of);
        }
        let mut rows:Vec<Value>=query.fetch_all(&mut *conn).await.map_err(db)?;
        let more=rows.len() as i64>page.limit;rows.truncate(page.limit as usize);
        let next=if more{rows.last().and_then(|r|r["id"].as_str())}else{None};
        metadata.push(json!({"id":table,"label":label,"hasMore":more,"nextCursor":next,"limit":page.limit,"included":rows.len(),"wholeFilteredCollection":!more&&page.cursor.is_none()}));
        result[table]=json!(rows);
    }
    let mut book_rows = result["books"].as_array().cloned().unwrap_or_default();
    if page.collection == Some("books") {
        book_rows.retain(|r| {
            page.selected.is_none_or(|id| r["id"] == id.to_string())
                && page.cursor.is_none_or(|id| {
                    r["id"]
                        .as_str()
                        .is_some_and(|v| v > id.to_string().as_str())
                })
                && format!(
                    "{} {}",
                    r["code"].as_str().unwrap_or_default(),
                    r["name"].as_str().unwrap_or_default()
                )
                .to_lowercase()
                .contains(&page.search.to_lowercase())
        });
        result["books"] = json!(book_rows);
    }
    metadata.insert(0,json!({"id":"books","label":"Official book","hasMore":false,"nextCursor":null,"limit":page.limit,"included":book_rows.len(),"wholeFilteredCollection":true}));
    result["collections"] = json!(metadata);
    result["pageLimit"] = json!(page.limit);
    if let Some(selected) = page.selected {
        let collection = page.collection.unwrap_or("journals");
        let kind = match collection {
            "books" => "book",
            "accounts" => "account",
            "periods" => "period",
            "invoice_policies" => "invoice_policy",
            "invoice_sources" => "invoice_source",
            "purchase_policies" => "purchase_policy",
            "purchase_sources" => "purchase_source",
            "stock_policies" => "stock_policy",
            "stock_sources" => "stock_source",
            "bank_accounts" => "bank_account",
            "bank_statements" => "bank_statement",
            "bank_rows" => "bank_row",
            "bank_allocations" => "bank_allocation",
            "bank_unmatches" => "bank_unmatch",
            "bank_policies" => "bank_policy",
            "bank_reconciliations" => "bank_reconciliation",
            "payables" => "purchase_source",
            "payable_settlements" => "payable_settlement",
            "clearing_sources" => "invoice_source",
            "clearing_settlements" => "clearing_settlement",
            "invoice_term_sources" => "invoice_source",
            "supplier_term_sources" => "purchase_source",
            "document_terms" => "document_terms",
            "tax_codes" => "tax_code",
            _ => "journal",
        };
        if matches!(
            collection,
            "invoice_term_sources" | "supplier_term_sources" | "document_terms"
        ) && !result[collection]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|r| r["id"] == selected.to_string()))
        {
            return Err(ErpError::NotFound.into());
        }
        let visible = if kind == "book" {
            selected == key
        } else {
            book_for(conn, tenant, kind, selected).await? == key
        };
        if !visible {
            return Err(ErpError::NotFound.into());
        }
        let mut detail = snapshot(conn, tenant, kind, selected).await?;
        if kind == "bank_allocation" {
            if let Some(amount) = detail["record"]["amount_minor"].as_i64() {
                detail["record"]["amount_minor"] = json!(amount.to_string());
            }
        }
        if let Some(lines) = detail["lines"].as_array_mut() {
            for l in lines {
                for key in ["debit_minor", "credit_minor"] {
                    if let Some(v) = l[key].as_i64() {
                        l[key] = json!(v.to_string());
                    }
                }
            }
        }
        let record = result[collection]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["id"] == selected.to_string()))
            .cloned()
            .unwrap_or_else(|| detail["record"].clone());
        let lines=detail["lines"].as_array().map(|rows|rows.iter().map(|r|json!({"accountCode":r["account_code"],"accountName":r["account_name"],"debitMinor":r["debit_minor"],"creditMinor":r["credit_minor"],"memo":r["memo"]})).collect::<Vec<_>>());
        // The immutable stock snapshot may contain 10,000 portions. Financial
        // review and provider lookup use the complete bounded account allocation,
        // never copy that operational evidence into an unbounded response.
        if kind == "stock_source" {
            detail = json!({"record":record});
        }
        result["detail"] = json!({"record":record,"lines":lines});
        result["selected"] = detail;
    }
    Ok(())
}

fn stock_projection() -> &'static str {
    "jsonb_build_object('id',s.id,'bookId',s.book_id,'code',s.effect||' · '||s.order_id::text,'orderId',s.order_id,'sourceRevision',s.source_revision,'effect',s.effect,'amountMinor',s.amount_minor::text,'sourceDate',s.source_date,'policyId',s.policy_id,'policyRevision',s.policy_revision,'revision',1,'originalCostPortionCount',jsonb_array_length(s.source_snapshot),'legs',coalesce((SELECT jsonb_agg(jsonb_build_object('accountId',l.account_id,'role',CASE WHEN EXISTS(SELECT 1 FROM jsonb_array_elements(s.segments) v WHERE v->>'inventoryAccountId'=l.account_id::text) THEN 'inventory' ELSE 'cost_of_goods_sold' END,'debitMinor',l.debit_minor::text,'creditMinor',l.credit_minor::text) ORDER BY l.account_id) FROM archava_accounting.stock_legs(s.tenant_id,s.id) l),'[]'::jsonb),'sourceState',CASE WHEN s.amount_minor=0 THEN 'no_monetary_effect' WHEN EXISTS(SELECT 1 FROM archava_accounting.stock_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted') THEN 'posted' WHEN EXISTS(SELECT 1 FROM archava_accounting.stock_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='draft') THEN 'draft' ELSE 'pending' END)"
}
