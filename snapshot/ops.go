package snapshot

import (
	"os"

	"google.golang.org/protobuf/proto"
)

func CreateSnapshot() *ProjectSnapshot {
	return &ProjectSnapshot{
		Files: map[string]*File{},
	}
}

func WriteSnapshot(snap *ProjectSnapshot, path string) error {
	// Proto encoding (3-5x faster than JSON)
	data, err := proto.Marshal(snap)
	if err != nil {
		return err
	}

	// Optional compression (zstd reduces 60-70%)
	// compressed := zstd.Compress(nil, data)

	return os.WriteFile(path, data, 0644)
}

func ReadSnapshot(path string) (*ProjectSnapshot, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	// Optional decompression
	// data = zstd.Decompress(nil, data)

	snap := &ProjectSnapshot{}
	return snap, proto.Unmarshal(data, snap)
}
