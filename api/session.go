package api

import (
	"JustSync/utils"
	"JustSync/websocket"
	"net/http"
)

func HandleConnectClient(w http.ResponseWriter, r *http.Request) {
	utils.LogInfo("Client connection request received")

	hub := websocket.GetHub()
	websocket.ServeWs(hub, w, r)

	utils.LogInfo("New client connected")
}
