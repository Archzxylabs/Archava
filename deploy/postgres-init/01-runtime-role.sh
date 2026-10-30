#!/bin/sh
# Runs once, on an empty PostgreSQL volume, before the owner migrates.
# The runtime role starts with CONNECT only; each ledger's grant function adds
# the schema/table privileges after its migrations have been applied.
set -eu

psql --set=ON_ERROR_STOP=1 \
     --username "$POSTGRES_USER" \
     --dbname "$POSTGRES_DB" \
     --set=runtime_password="$ARCHAVA_RUNTIME_PASSWORD" \
     --set=dbname="$POSTGRES_DB" <<'SQL'
CREATE ROLE archava_runtime LOGIN PASSWORD :'runtime_password';
GRANT CONNECT ON DATABASE :"dbname" TO archava_runtime;
SQL
