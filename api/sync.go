package api

import (
	"JustSync/entities"
	"JustSync/service"
	"JustSync/snapshot"
	"JustSync/utils"
	"bytes"
	"encoding/json"
	"log/slog"
	"net/http"
	"os"
)

func RequestSync(w http.ResponseWriter, r *http.Request) {
	slog.Info("Sync requested")

	var body entities.PathRequest
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		slog.Error("Invalid json body data provided")
		return
	}

	// PERF: Consider streaming file content instead of loading full content into memory. However for now, as we are mostly working with <1mb files, this is still fine
	content, err := os.ReadFile(body.Path)

	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		slog.Error("Could not read file data: " + err.Error())
		return
	}

	hash := utils.CreateBlake3Hash(content)
	snap, err := snapshot.ReadSnapshot("snapshot/SNAPSHOT.sync.snap")

	if err != nil {
		http.Error(w, err.Error(), http.StatusNotAcceptable)
		slog.Error("Snapshot not found or corrupted, maybe restart the session? " + err.Error())
		return
	}

	if bytes.Equal(hash, snap.Files[body.Path].WholeHash) {
		slog.Info("Sync request rejected, no change in file detected.")
		return
	}

	service.SyncAllClients(content, hash)
	w.WriteHeader(http.StatusOK)
}
