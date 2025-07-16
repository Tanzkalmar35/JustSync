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
	"time"

	"github.com/restic/chunker"
	"github.com/zeebo/blake3"
	"gopkg.in/yaml.v3"
)

const (
	MinChunkSize = 4 * 1024         // 4kb
	AvgChunkSize = 16 * 1024        // 16kb
	MaxChunkSize = 13               // 2 to the power of 13 in practise
	ChunkerPol   = 0x3DA3358B4DC173 // Recommended CDC polynomial
)

func ProcessDir(root string) (*snapshot.ProjectSnapshot, error) {
	snap := &snapshot.ProjectSnapshot{
		Version:   "1.0",
		Timestamp: time.Now().UnixNano(),
		Files:     map[string]*snapshot.FileChunks{},
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

		filesnap, e := processFile(path)

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

func processFile(path string) (snapshot.FileChunks, error) {
	snap := snapshot.FileChunks{
		WholeHash: []byte{},
		Chunks:    []*snapshot.Chunk{},
	}

	file, err := os.Open(path)
	if err != nil {
		return snap, err
	}
	defer file.Close()
	content, err := io.ReadAll(file)
	if err != nil {
		LogError("Error while reading content of file %s: %s", file, err.Error())
		return snap, err
	}

	// Hash whole content
	snap.WholeHash = GetHasher()(content)

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
func ChunkFileContentDefined(file io.Reader) ([]*snapshot.Chunk, error) {
	hasher := GetHasher()
	var chunks []*snapshot.Chunk
	offset := int64(0)

	chkr := chunker.NewWithBoundaries(file, chunker.Pol(ChunkerPol), MinChunkSize, MaxChunkSize)
	chkr.SetAverageBits(MaxChunkSize)

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

		hash := hasher(c.Data)
		size := int64(len(c.Data))

		chunk := snapshot.Chunk{
			Hash:   hash,
			Offset: offset,
			Size:   size,
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

func GetOsSpecificConfigPath() string {
	switch runtime.GOOS {
	case "windows": // Well... windows
		return filepath.Join(os.Getenv("APPDATA"), "JustSync")
	case "darwin": // Macos
		return filepath.Join(os.Getenv("HOME"), "Library", "Application Support", "JustSync")
	default: // Linux, BSD, ...
		if xdg := os.Getenv("XDG_CONFIG_HOME"); xdg != "" {
			return filepath.Join(xdg, "JustSync")
		}
		return filepath.Join(os.Getenv("HOME"), ".config", "JustSync")
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

func GetExternalClientConfig(name string) ExternalClientConfig {
	var config ExternalClientConfig
	path := filepath.Join(GetOsSpecificConfigPath(), name+".yml")
	configContent, err := os.ReadFile(path)
	if err != nil {
		LogError("Config '%s' not found at os' specific config path '%s'", name, path)
		return config
	}

	if err = yaml.Unmarshal(configContent, &config); err != nil {
		LogError("Error in config '%s' found. Could not parse config.", name)
		return config
	}

	return config
}

func GetExternalHostConfig(name string) ExternalHostConfig {
	var config ExternalHostConfig
	path := filepath.Join(GetOsSpecificConfigPath(), name+".yml")
	configContent, err := os.ReadFile(path)
	if err != nil {
		LogError("Config '%s' not found at os' specific config path '%s'", name, path)
		return config
	}

	if err = yaml.Unmarshal(configContent, &config); err != nil {
		LogError("Error in config '%s' found. Could not parse config.", name)
		return config
	}

	return config
}
