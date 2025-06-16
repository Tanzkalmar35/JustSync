package service

import "JustSync/utils"

func HandleCreateSnapshot(path string) error {
	utils.ProcessDir(path)
	// 1. Compress files
	// 2. Store compressed snapshot
	return nil
}
