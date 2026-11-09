# Storage

Content-addressable storage uses SHA-256 blobs outside SQLite:

`storage/projects/{project_id}/blobs/`

Logical files and immutable versions remain separate from blobs. Identical content reuses an existing blob; different logical files may reference the same blob. Originals are never overwritten.
