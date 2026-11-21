-- Runs only on first initialization of the PostgreSQL volume.
-- Credentials are supplied through environment variables by the deployment host.
CREATE USER keycloak_user WITH PASSWORD 'CHANGE_ME_KEYCLOAK_DB_PASSWORD';
CREATE DATABASE keycloak OWNER keycloak_user;
