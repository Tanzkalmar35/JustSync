package snapshot

import (
	"os"
	"time"

	"google.golang.org/protobuf/proto"
)

func CreateSnapshot() *ProjectSnapshot {
	return &ProjectSnapshot{
		Version:   "1.0",
		Timestamp: time.Now().UnixNano(),
		Files:     map[string]*FileChunks{
			// "src/main.go": {
			// 	WholeHash:   createBlake3Hash(32), // Replace with real BLAKE3
			// 	ChunkHashes: [][]byte{createBlake3Hash(32), createBlake3Hash(32)},
			// },
		},
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
