ALTER TABLE archava_commerce.products ADD COLUMN managed_by text CHECK(managed_by='foodservice');
ALTER TABLE archava_commerce.products ADD CONSTRAINT managed_product_policy CHECK(managed_by IS NULL OR (unit='unit' AND NOT track_stock));
CREATE OR REPLACE FUNCTION archava_commerce.guard_product() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.sku IS DISTINCT FROM OLD.sku OR NEW.title IS DISTINCT FROM OLD.title OR NEW.unit IS DISTINCT FROM OLD.unit OR NEW.track_stock IS DISTINCT FROM OLD.track_stock OR NEW.currency IS DISTINCT FROM OLD.currency OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.managed_by IS DISTINCT FROM OLD.managed_by OR NEW.revision<>OLD.revision+1
 THEN RAISE EXCEPTION 'Catalog identity/revision is immutable' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
