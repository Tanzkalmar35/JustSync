package snapshot

import (
	"os"
	"sync"

	"google.golang.org/protobuf/proto"
)

const (
	SnapPath string = "snapshot/SNAPSHOT.sync.snap"
)

var (
	snapshot   *ProjectSnapshot
	snapshotMu sync.Mutex
)

func GetSnapshot() *ProjectSnapshot {
	return snapshot
}

func WriteSnapshot(snap *ProjectSnapshot) error {
	snapshotMu.Lock()
	defer snapshotMu.Unlock()

	data, err := proto.Marshal(snap)
	if err != nil {
		return err
	}

	snapshot = snap

	return os.WriteFile(SnapPath, data, 0644)
}
