package service

import (
	"JustSync/snapshot"
	"JustSync/utils"
)

func HandleCreateSnapshot(path string) error {
	snappath := "snapshot/SNAPSHOT.sync.snap"

	snap, err := utils.ProcessDir(path)

	if err != nil {
		return err
	}

	snapshot.WriteSnapshot(snap, snappath)

	utils.LogInfo("Created new snapshot at: %s", snappath)
	return nil
}
