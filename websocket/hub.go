package websocket

import (
	"JustSync/utils"
	"net/http"
	"slices"
	"strconv"
	"sync"

	"github.com/gorilla/websocket"
)

var (
	Upgrader = websocket.Upgrader{
		ReadBufferSize:  1024,
		WriteBufferSize: 1024,

		CheckOrigin: CheckOrigin,
	}
	instance       *Hub
	once           sync.Once
	allowedOrigins = []string{"sync.fabianholler.live"}
)

func CheckOrigin(r *http.Request) bool {
	origin := r.Header.Get("Origin")

	// As the origin header is a browser thing, requests from machine clients do not have this header.
	// So we just allow there
	if origin == "" {
		return true
	}

	if slices.Contains(allowedOrigins, origin) {
		return true
	}

	utils.LogWarn("Connection attempt from not whitelisted url: %s", origin)
	return false
}

type Hub struct {
	Clients   map[*Client]bool
	Broadcast chan []byte

	register   chan *Client
	unregister chan *Client
	mu         sync.RWMutex
}

func GetHub() *Hub {
	once.Do(func() {
		instance = &Hub{
			Clients:   make(map[*Client]bool),
			Broadcast: make(chan []byte),

			register:   make(chan *Client),
			unregister: make(chan *Client),
		}
		utils.LogInfo("Starting hub")
		go instance.Run()
	})

	return instance
}

func (h *Hub) isRegistered(client *Client) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	_, ok := h.Clients[client]
	return ok
}

func (h *Hub) Run() {
	for {
		select {

		// Register client
		case client := <-h.register:
			h.mu.Lock()
			h.Clients[client] = true
			utils.LogInfo("Registered client %s", strconv.Itoa(len(h.Clients)))
			h.mu.Unlock()

		// Unregister client
		case client := <-h.unregister:
			h.mu.Lock()
			if _, ok := h.Clients[client]; ok {
				delete(h.Clients, client)
				close(client.send)
				utils.LogInfo("Unregistered client")
			} else {
				utils.LogError("Error while unregistering client")
			}
			h.mu.Unlock()

		// Message received, broadcast it to all clients
		case message := <-h.Broadcast:
			utils.LogInfo("Broadcasting message")
			h.mu.Lock()
			for client := range h.Clients {
				select {
				case client.send <- message:
					utils.LogInfo("Message broadcasted: %s", message)
				default:
					// Fall back. Close and disconnect everything in case the client's send buffer is full or it is dead or stuck
					utils.LogError("Broadcast failed - maybe the buffer of one of the clients is full or it is dead or stuck")
					close(client.send)
					delete(h.Clients, client)
				}
			}
			h.mu.Unlock()
		}
	}
}
