# Database

SQLite is project-isolated at `storage/projects/{project_id}/db.sqlite`.

Planned core relations:
- projects
- files
- file_versions
- blobs
- token_counts
- operations

Migrations must run transactionally and be verified before marking complete. WAL is planned for normal operation.
