//! Text ordering in a disposable cluster must not depend on the machine.
//!
//! The fixture used to inherit the caller's locale, so `ORDER BY` on a text
//! column sorted `INVENTORY` before `INV-NEW` under en_US and after it under
//! C. A suite that asserted one order passed on one machine and failed on
//! another. `provision` now pins `lc-collate` to `C`; this proves the effect
//! on a real cluster rather than the flag's presence in the command line.

#![cfg(unix)]

use archava_pg_fixture::provision;

#[tokio::test]
async fn text_ordering_is_byte_order_whatever_the_callers_locale_is() {
    let fixture = provision().await;
    let (collation,): (String,) =
        sqlx::query_as("SELECT datcollate FROM pg_database WHERE datname = current_database()")
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("read the database collation");
    assert_eq!(collation, "C");
    let ordered: Vec<(String,)> = sqlx::query_as(
        "SELECT code FROM (VALUES ('INVENTORY'), ('INV-NEW'), ('inv'), ('GRNI')) AS t(code) ORDER BY code",
    )
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("order the probe rows");
    let codes: Vec<&str> = ordered.iter().map(|(code,)| code.as_str()).collect();
    assert_eq!(codes, ["GRNI", "INV-NEW", "INVENTORY", "inv"]);
}
