package utils

import (
	"JustSync/snapshot"
	"crypto/rand"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"time"

	"google.golang.org/protobuf/proto"
)

func ProcessDir(root string) error {
	if info, err := os.Stat(root); err != nil {
		return fmt.Errorf("Invalid path: %w", err)
	} else if !info.IsDir() {
		return errors.New("Path does not point to a directory")
	}

	return filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		// Handle directory traversal errors
		if err != nil {
			return fmt.Errorf("access error at %s: %w", path, err)
		}

		// Skip directories
		if d.IsDir() {
			return nil
		}

		// Process file (replace this with your actual logic)
		if err := processFile(path); err != nil {
			// Handle but don't abort on file processing errors
			return fmt.Errorf("processing error: %v\n", err)
		}

		return nil
	})
}

func processFile(path string) error {
	content, err := os.ReadFile(path)

	if err != nil {
		return err
	}

	fmt.Printf("\n═════ File: %s ═════\n", path)
	fmt.Println(string(content))
	fmt.Println("═══════════════════════════════════════════════")

	return nil
}

func CreateSnapshot() *snapshot.ProjectSnapshot {
	return &snapshot.ProjectSnapshot{
		Version:   "1.0",
		Timestamp: time.Now().UnixNano(),
		Files: map[string]*snapshot.FileChunks{
			"src/main.go": {
				WholeHash:   randomBytes(32), // Replace with real BLAKE3
				ChunkHashes: [][]byte{randomBytes(32), randomBytes(32)},
			},
		},
	}
}

// Write snapshot to file (optimized)
func WriteSnapshot(snap *snapshot.ProjectSnapshot, path string) error {
	// Proto encoding (3-5x faster than JSON)
	data, err := proto.Marshal(snap)
	if err != nil {
		return err
	}

	// Optional compression (zstd reduces 60-70%)
	// compressed := zstd.Compress(nil, data)

	return os.WriteFile(path, data, 0644)
}

// Read snapshot from file (high-performance)
func ReadSnapshot(path string) (*snapshot.ProjectSnapshot, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	// Optional decompression
	// data = zstd.Decompress(nil, data)

	snap := &snapshot.ProjectSnapshot{}
	return snap, proto.Unmarshal(data, snap)
}

// Helper for demo (replace with real hashing)
func randomBytes(n int) []byte {
	b := make([]byte, n)
	rand.Read(b)
	return b
}
