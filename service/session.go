package service

import (
	"JustSync/snapshot"
	"JustSync/utils"
	"log/slog"
)

func HandleCreateSnapshot(path string) error {
	snappath := "snapshot/SNAPSHOT.sync.snap"

	snap, err := utils.ProcessDir(path)

	if err != nil {
		return err
	}

	snapshot.WriteSnapshot(snap, snappath)

	slog.Info("Created new snapshot at " + snappath)
	return nil
}
