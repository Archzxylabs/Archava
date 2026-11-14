import test from 'node:test'
import assert from 'node:assert/strict'
import { operationInventory, rustSuites } from '../product/audit.mjs'

test('operation inventory keeps field renames out of the command denominator',()=>{
  const source=`pub enum SubscriptionCommand {
    #[serde(rename = "create_subscription")]
    CreateSubscription {
        #[serde(rename = "commandId")]
        command_id: String,
    },
    ChangeStatus { command_id: String },
}`
  assert.deepEqual(operationInventory(source,'subscriptions'),['create_subscription','change_status'])
})
test('operation inventory uses canonical command names without treating permissions as actions',()=>{
  const source=`impl SupportCommand {
    pub fn operation(&self) -> &'static str {
      match self { Self::Create { .. } => "case.create", Self::Wait { .. } => "case.wait" }
    }
    pub fn permission(&self) -> &'static str { "cases.write" }
  }`
  assert.deepEqual(operationInventory(source,'support'),['case.create','case.wait'])
})
test('executed Rust evidence keeps failed and ignored tests distinct from passing suites',()=>{
  const suites=rustSuites(`     Running tests/crm.rs (target/debug/deps/crm-123)
running 3 tests
test good ... ok
test bad ... FAILED
test omitted ... ignored
test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured;
     Running unittests src/lib.rs (target/debug/deps/archava_slice-123)
test unrelated ... ok
test result: ok. 1 passed; 0 failed; 0 ignored;
     Running tests/own.rs (target/debug/deps/own-123)
test parity ... ok
test result: ok. 1 passed; 0 failed; 0 ignored;`)
  assert.equal(suites.crm.status,'FAILED');assert.equal(suites.crm.failed,1);assert.equal(suites.crm.ignored,1)
  assert.deepEqual(suites.crm.tests.map(t=>t.name),['good','bad','omitted'])
  assert.equal(suites.own.passed,1);assert.equal(suites.own.tests[0].name,'parity')
})
