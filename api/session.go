package api

import (
	"JustSync/service"
	"JustSync/utils"
	"JustSync/websocket"
	"encoding/json"
	"net/http"
	"strconv"
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

	utils.LogInfo("Client %s connected", strconv.Itoa(len(hub.Clients)))
}

//
// var req struct{ Otp, Hostname string }
//
// err := json.NewDecoder(r.Body).Decode(&req)
// if err != nil {
// 	http.Error(w, err.Error(), http.StatusBadRequest)
// 	utils.LogError("Invalid json body data given")
// 	return
// }
//
// if !utils.GetTokenManager().ValidateOtp(req.Otp) {
// 	msg := "Given one time password could not be validated. Remember these invalidate after 10mins"
// 	http.Error(w, msg, http.StatusForbidden)
// 	utils.LogError(msg)
// 	return
// }
//
// sessionToken := utils.GetTokenManager().GenerateToken()
//
// type AUthResponse struct {
// 	SessionToken string `json:"session_token"`
// }
//
// // Create http response containing the session token
// response := AUthResponse{SessionToken: sessionToken}
// w.Header().Set("Content-Type", "application/json")
// w.WriteHeader(http.StatusOK)
// json.NewEncoder(w).Encode(response)
