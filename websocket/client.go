package websocket

import (
	"JustSync/service"
	"JustSync/utils"
	"fmt"
	"net/http"
	"time"

	"github.com/gorilla/websocket"
	"google.golang.org/protobuf/proto"
)

const (
	handshakeWait = 5 * time.Second
)

type Client struct {
	hub  *Hub
	conn *websocket.Conn
	send chan []byte
}

func (c *Client) readPump() {
	defer func() {
		c.hub.unregister <- c
		c.conn.Close()
	}()

	for {
		_, msg, err := c.conn.ReadMessage()
		if err != nil {
			// Client disconnected
			break
		}

		c.hub.Broadcast <- msg
	}
}

func (c *Client) writePump() {
	defer c.conn.Close()

	for {
		select {
		case msg, ok := <-c.send:
			if !ok {
				c.conn.WriteMessage(websocket.CloseMessage, []byte{})
				return
			}
			c.conn.WriteMessage(websocket.TextMessage, msg)
		}
	}
}

func (c *Client) handleConnectionPreparation(w http.ResponseWriter) {
	defer func() {
		if c.hub.isRegistered(c) {
			c.conn.Close()
		}
	}()

	if err := c.ExecuteHandshake(); err != nil {
		w.WriteHeader(http.StatusForbidden)
		return
	}
	c.DoFullProjectSync()
	c.readPump()
}

func (c *Client) ExecuteHandshake() error {
	c.conn.SetReadDeadline(time.Now().Add(handshakeWait))

	msgType, msg, err := c.conn.ReadMessage()

	if err != nil {
		utils.LogError("Handshake failed: Could not read auth token")
		return err
	}

	if msgType != websocket.TextMessage {
		utils.LogError("Handshake failed: Auth token must be a text message")
		return fmt.Errorf("Handshake failed: Auth token must be a text message")
	}

	token := string(msg)
	if !utils.GetTokenManager().ValidateOtp(token) {
		utils.LogError("Handshake failed: Invalid auth token received")
		return fmt.Errorf("Handshake failed: Invalid auth token received")
	}

	utils.LogInfo("Handshake successful")

	c.conn.SetReadDeadline(time.Time{})
	c.hub.register <- c

	return nil
}

func (c *Client) DoFullProjectSync() error {
	msgs, err := service.PrepareInitiateProjectSync()
	if err != nil {
		utils.LogError("Failed to initiate project sync to client due to: %s", err.Error())
		return err
	}

	for _, msg := range msgs {
		content, err := proto.Marshal(&msg)
		if err != nil {
			utils.LogError("Could not marshall file %s", msg.GetFile().Path)
			return err
		}
		c.send <- content
	}

	return nil
}

func ServeWs(hub *Hub, w http.ResponseWriter, r *http.Request) {
	conn, err := Upgrader.Upgrade(w, r, nil)
	if err != nil {
		utils.LogError("Error attempting to build websocket connection: %s", err.Error())
		return
	}

	client := &Client{hub: hub, conn: conn, send: make(chan []byte, 256)}

	// Start read and write pumps
	go client.writePump()
	go client.handleConnectionPreparation(w)
}
