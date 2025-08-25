package utils

import (
	"JustSync/snapshot"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strconv"

	"github.com/restic/chunker"
	"github.com/zeebo/blake3"
	"gopkg.in/yaml.v3"
)

const (
	MinChunkSize = 1 << 11          // 2kb
	AvgChunkSize = 1 << 13          // 8kb MaxChunkSize = 1 << 15          // 32 kb
	ChunkerPol   = 0x3DA3358B4DC173 // Recommended CDC polynomial
)

// TODO: Refactor to make modular
func CreateSnapshotOfDir(root string) (*snapshot.ProjectSnapshot, error) {
	snap := &snapshot.ProjectSnapshot{
		Files: map[string]*snapshot.InitialSyncFile{},
	}
	if info, err := os.Stat(root); err != nil {
		return snap, fmt.Errorf("Invalid path: %w", err)
	} else if !info.IsDir() {
		return snap, errors.New("Path does not point to a directory")
	}

	if err := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return fmt.Errorf("access error at %s: %w", path, err)
		}

		// Skip directories
		if d.IsDir() {
			return nil
		}

		filesnap, e := CreateSnapshotOfFile(path)

		if e != nil {
			// Handle but don't abort on file processing errors
			return fmt.Errorf("processing error: %v\n", err)
		}

		snap.Files[path] = &filesnap

		return nil
	}); err != nil {
		return snap, err
	}

	return snap, nil
}

func CreateSnapshotOfFile(path string) (snapshot.InitialSyncFile, error) {
	snap := snapshot.InitialSyncFile{}

	file, err := os.Open(path)
	if err != nil {
		return snap, err
	}
	defer file.Close()
	content, err := io.ReadAll(file)
	if err != nil {
		LogError("Error while reading content of file %s: %s", file.Name(), err.Error())
		return snap, err
	}

	// Hash whole content
	snap.Checksum = GetHasher()(content)

	// Reset the file pointer to the beginning of the file before chunking
	_, err = file.Seek(0, io.SeekStart)
	if err != nil {
		return snap, fmt.Errorf("failed to seek file: %w", err)
	}

	// Split into chunks and hash these
	chunkHashes, err := ChunkFileContentDefined(file)
	if err != nil {
		return snap, err
	}

	snap.Chunks = chunkHashes

	LogDebug(strconv.Itoa(len(snap.Chunks)))
	return snap, nil
}

// ChunkFileContentDefined chunks files using CDC
func ChunkFileContentDefined(file io.Reader) ([]*snapshot.InitialSyncChunk, error) {
	hasher := GetHasher()
	var chunks []*snapshot.InitialSyncChunk
	offset := int64(0)

	chkr := chunker.NewWithBoundaries(file, chunker.Pol(ChunkerPol), MinChunkSize, MaxChunkSize)
	chkr.SetAverageBits(AvgChunkSize)

	buf := make([]byte, MaxChunkSize)
	for {
		c, err := chkr.Next(buf)
		if err == io.EOF {
			break
		}
		if err != nil {
			LogError("An error occured while attempting to CDC chunk file %s: %s", file, err.Error())
			return nil, err
		}

		size := int64(len(c.Data))

		chunk := snapshot.InitialSyncChunk{
			Checksum: hasher(c.Data),
			Content:  c.Data,
			Offset:   offset,
			Size:     size,
		}
		chunks = append(chunks, &chunk)

		offset += size
	}

	return chunks, nil
}

// GetHasher returns a hashing function using blake3 algorithm.
func GetHasher() func([]byte) []byte {
	return func(data []byte) []byte {
		hash := blake3.Sum256(data)
		return hash[:]
	}
}

func CreateConfigFolderAt(path string) {
	if _, err := os.Stat(path); os.IsNotExist(err) {
		if err := os.MkdirAll(path, 0755); err != nil {
			LogError("Config could not be initialized at %s due to %s", path, err.Error())
		} else {
			LogInfo("Created config at %s", path)
		}
	} else {
		LogInfo("Config directory does already exist")
	}
}
