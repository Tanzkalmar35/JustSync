# Snapshot Viewer Tool

Helpful for verifying snapshot file content when debugging.

## Build and use the snapshot viewer

When inside the project root, simply run

```bash
go run cmd/viewer/viewer.go [relative/pathto/snapshotfile]
```

and it will print the decoded content in json syntax.
