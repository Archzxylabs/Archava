-- The native embedded migration profile must advance with provider disclosure.
-- Earlier binaries ignore additive Owner JSON fields and must not serve this
-- profile, even when all financial tables are otherwise backward compatible.
CREATE FUNCTION archava_owner.ai_disclosure_boundary_version()
RETURNS text LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path=pg_catalog
AS $$ SELECT 'archava-business-provider-context/v2'::text $$;
