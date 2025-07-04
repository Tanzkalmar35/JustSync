package websocket

import (
	"JustSync/utils"
	"net/http"
	"sync"

	"github.com/gorilla/websocket"
)

var (
	Upgrader = websocket.Upgrader{
		ReadBufferSize:  1024,
		WriteBufferSize: 1024,

		// TODO:
		// Checks the origin of the connection.
		CheckOrigin: func(r *http.Request) bool { return true },
	}
	instance *Hub
	once     sync.Once
)

type Hub struct {
	Clients   map[*Client]bool
	broadcast chan []byte

	register   chan *Client
	unregister chan *Client
}

func GetHub() *Hub {
	once.Do(func() {
		instance = &Hub{
			Clients:   make(map[*Client]bool),
			broadcast: make(chan []byte),

			register:   make(chan *Client),
			unregister: make(chan *Client),
		}
		go instance.Run()
	})

	return instance
}

func (h *Hub) Run() {
	for {
		select {
		// Register client
		case client := <-h.register:
			h.Clients[client] = true
		// Unregister client
		case client := <-h.unregister:
			if _, ok := h.Clients[client]; ok {
				delete(h.Clients, client)
				close(client.send)
			}
		// Message received, broadcast it to all clients
		case message := <-h.broadcast:
			for client := range h.Clients {
				select {
				case client.send <- message:
					utils.LogInfo("Message broadcasted: %s", message)
				default:
					// Fall back. Close and disconnect everything in case the client's send buffer is full or it is dead or stuck
					close(client.send)
					delete(h.Clients, client)
				}
			}
		}
	}
}
