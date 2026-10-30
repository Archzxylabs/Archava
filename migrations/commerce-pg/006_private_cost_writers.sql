-- Only ledger-owned physical movement/return triggers may write valuation rows.
-- Trigger depth alone does not exclude a runtime-owned temporary trigger.
ALTER FUNCTION archava_commerce.cost_movement() SECURITY DEFINER;
ALTER FUNCTION archava_commerce.cost_return() SECURITY DEFINER;
REVOKE ALL ON FUNCTION archava_commerce.cost_movement(),archava_commerce.cost_return() FROM PUBLIC;

DO $$
DECLARE grant_role text;
BEGIN
 -- Upgrade existing installations as well as reducing fresh-install grants.
 FOR grant_role IN
  SELECT DISTINCT r.rolname FROM pg_class c
  JOIN pg_namespace n ON n.oid=c.relnamespace
  CROSS JOIN LATERAL aclexplode(c.relacl) a
  JOIN pg_roles r ON r.oid=a.grantee
  WHERE n.nspname='archava_commerce'
   AND c.relname IN('inventory_layers','inventory_uses','inventory_return_costs')
   AND a.grantee<>c.relowner
 LOOP
  EXECUTE format('REVOKE INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER ON archava_commerce.inventory_layers,archava_commerce.inventory_uses,archava_commerce.inventory_return_costs FROM %I',grant_role);
 END LOOP;
 FOR grant_role IN
  SELECT DISTINCT r.rolname FROM pg_class c
  JOIN pg_namespace n ON n.oid=c.relnamespace
  CROSS JOIN LATERAL aclexplode(c.relacl) a
  JOIN pg_roles r ON r.oid=a.grantee
  WHERE n.nspname='archava_commerce' AND c.relkind='S'
   AND c.relname IN('inventory_layers_id_seq','inventory_uses_id_seq','inventory_return_costs_id_seq')
   AND a.grantee<>c.relowner
 LOOP
  EXECUTE format('REVOKE ALL ON SEQUENCE archava_commerce.inventory_layers_id_seq,archava_commerce.inventory_uses_id_seq,archava_commerce.inventory_return_costs_id_seq FROM %I',grant_role);
 END LOOP;
 FOR grant_role IN
  SELECT DISTINCT r.rolname FROM pg_proc p
  JOIN pg_namespace n ON n.oid=p.pronamespace
  CROSS JOIN LATERAL aclexplode(p.proacl) a
  JOIN pg_roles r ON r.oid=a.grantee
  WHERE n.nspname='archava_commerce' AND p.proname IN('cost_movement','cost_return')
   AND a.grantee<>p.proowner
 LOOP
  EXECUTE format('REVOKE ALL ON FUNCTION archava_commerce.cost_movement(),archava_commerce.cost_return() FROM %I',grant_role);
 END LOOP;
END $$;
REVOKE INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER ON archava_commerce.inventory_layers,archava_commerce.inventory_uses,archava_commerce.inventory_return_costs FROM PUBLIC;
REVOKE ALL ON SEQUENCE archava_commerce.inventory_layers_id_seq,archava_commerce.inventory_uses_id_seq,archava_commerce.inventory_return_costs_id_seq FROM PUBLIC;
