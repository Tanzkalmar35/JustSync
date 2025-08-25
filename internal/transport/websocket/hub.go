package websocket

import (
	"JustSync/pkg"
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

	pkg.LogWarn("Connection attempt from not whitelisted url: %s", origin)
	return false
}

type Hub struct {
	Peers     map[*Peer]bool
	Broadcast chan []byte

	register   chan *Peer
	unregister chan *Peer
	mu         sync.RWMutex
}

func GetHub() *Hub {
	once.Do(func() {
		instance = &Hub{
			Peers:     make(map[*Peer]bool),
			Broadcast: make(chan []byte),

			register:   make(chan *Peer),
			unregister: make(chan *Peer),
		}
		pkg.LogInfo("Starting hub")
		go instance.Run()
	})

	return instance
}

func (h *Hub) isRegistered(client *Peer) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	_, ok := h.Peers[client]
	return ok
}

func (h *Hub) Run() {
	for {
		select {

		// Register client
		case client := <-h.register:
			h.mu.Lock()
			h.Peers[client] = true
			pkg.LogInfo("Registered client %s", strconv.Itoa(len(h.Peers)))
			h.mu.Unlock()

		// Unregister client
		case client := <-h.unregister:
			h.mu.Lock()
			if _, ok := h.Peers[client]; ok {
				delete(h.Peers, client)
				close(client.send)
				pkg.LogInfo("Unregistered client")
			} else {
				pkg.LogError("Error while unregistering client")
			}
			h.mu.Unlock()

		// Message received, broadcast it to all clients
		case message := <-h.Broadcast:
			pkg.LogInfo("Broadcasting message")
			h.mu.Lock()
			for client := range h.Peers {
				select {
				case client.send <- message:
					pkg.LogInfo("Message broadcasted")
				default:
					// Fall back. Close and disconnect everything in case the client's send buffer is full or it is dead or stuck
					pkg.LogError("Broadcast failed - maybe the buffer of one of the clients is full or it is dead or stuck")
					close(client.send)
					delete(h.Peers, client)
				}
			}
			h.mu.Unlock()
		}
	}
}
