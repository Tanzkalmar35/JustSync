package api

import (
	"JustSync/service"
	"JustSync/snapshot"
	"JustSync/utils"
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
)

func RequestSync(w http.ResponseWriter, r *http.Request) {
	utils.LogInfo("Sync requested")

	var body struct{ path string }
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		utils.LogError("Invalid json body data provided")
		return
	}

	// PERF: Consider streaming file content instead of loading full content into memory. However for now, as we are mostly working with <1mb files, this is still fine
	content, err := os.ReadFile(body.path)

	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		utils.LogError("Could not read file data: " + err.Error())
		return
	}

	hash := utils.CreateBlake3Hash(content)
	snap, err := snapshot.ReadSnapshot("snapshot/SNAPSHOT.sync.snap")

	if err != nil {
		http.Error(w, err.Error(), http.StatusNotAcceptable)
		utils.LogError("Snapshot not found or corrupted, maybe restart the session? " + err.Error())
		return
	}

	if bytes.Equal(hash, snap.Files[body.path].WholeHash) {
		utils.LogInfo("Sync request rejected, no change in file detected.")
		return
	}

	service.SyncAllClients(content, hash)
	w.WriteHeader(http.StatusOK)
}

func HeartBeat(w http.ResponseWriter, r *http.Request) {
	utils.LogInfo("Heartbeat received")

	w.WriteHeader(http.StatusOK)
	fmt.Fprintln(w, "Heartbeat successful")
}
