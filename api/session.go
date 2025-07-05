package api

import (
	"JustSync/service"
	"JustSync/utils"
	"JustSync/websocket"
	"encoding/json"
	"net/http"
)

// Accepts json data
func Setup(w http.ResponseWriter, r *http.Request) {
	utils.LogInfo("Setup requested")

	var req struct{ path string }
	err := json.NewDecoder(r.Body).Decode(&req)

	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		utils.LogError("Invalid json body data given")
		return
	}

	err = service.HandleCreateSnapshot(req.path)

	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		utils.LogError("Could not create snapshot, probably an invalid path")
		return
	}

	utils.LogInfo("Setup successful")
	w.WriteHeader(http.StatusOK)
}

func HandleConnectClient(w http.ResponseWriter, r *http.Request) {
	utils.LogInfo("Client connection request received")

	hub := websocket.GetHub()
	websocket.ServeWs(hub, w, r)

	utils.LogInfo("New client connected")
}
