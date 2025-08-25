package websocket

import (
	"JustSync/internal/service"
	"JustSync/pkg"
	"fmt"
	"net/http"
	"time"

	"github.com/gorilla/websocket"
	"google.golang.org/protobuf/proto"
)

var hostConn *websocket.Conn

const (
	handshakeWait = 5 * time.Second
	pingInterval  = 60 * time.Second
)

type Peer struct {
	hub  *Hub
	Conn *websocket.Conn
	send chan []byte
}

func SetHostConnection(conn *websocket.Conn) {
	hostConn = conn
}

func GetHostConnection() *websocket.Conn {
	return hostConn
}

func (p *Peer) readPump() {
	defer func() {
		p.hub.unregister <- p
		p.Conn.Close()
	}()

	for {
		_, msg, err := p.Conn.ReadMessage()
		if err != nil {
			// Client disconnected
			break
		}
		pkg.LogInfo("Message received")

		p.hub.Broadcast <- msg
	}
}

func (p *Peer) writePump() {
	ticker := time.NewTicker(pingInterval)
	defer func() {
		ticker.Stop()
		p.Conn.Close()
	}()
	for {
		select {
		case msg, ok := <-p.send:
			if !ok {
				if err := p.Conn.WriteMessage(websocket.CloseMessage, []byte{}); err != nil {
					pkg.LogError("Error writing close message to peer.")
				}
				return
			}
			p.Conn.WriteMessage(websocket.TextMessage, msg)
		case <-ticker.C:
			pkg.LogInfo("Pinging client")
			if err := p.Conn.WriteMessage(websocket.PingMessage, nil); err != nil {
				pkg.LogError("Could not send ping message to client")
				return
			}
		}
	}
}

func (p *Peer) handleConnectionPreparation() {
	defer func() {
		if p.hub.isRegistered(p) {
			p.Conn.Close()
		}
	}()

	if err := p.ExecuteHandshake(); err != nil {
		p.Conn.Close()
		return
	}
	p.DoFullProjectSync()
	p.readPump()
}

func (p *Peer) ExecuteHandshake() error {
	p.Conn.SetReadDeadline(time.Now().Add(handshakeWait))

	msgType, msg, err := p.Conn.ReadMessage()

	if err != nil {
		pkg.LogError("Handshake failed: Could not read auth token")
		return err
	}

	if msgType != websocket.TextMessage {
		pkg.LogError("Handshake failed: Auth token must be a text message")
		return fmt.Errorf("Handshake failed: Auth token must be a text message")
	}

	token := string(msg)
	if !service.GetTokenManager().ValidateOtp(token) {
		pkg.LogError("Handshake failed: Invalid auth token received")
		return fmt.Errorf("Handshake failed: Invalid auth token received")
	}

	pkg.LogInfo("Handshake successful")

	p.Conn.SetReadDeadline(time.Time{})
	p.hub.register <- p

	return nil
}

func (p *Peer) DoFullProjectSync() error {
	msgs, err := service.PrepareInitiateProjectSync()
	if err != nil {
		pkg.LogError("Failed to initiate project sync to client due to: %s", err.Error())
		return err
	}

	for _, msg := range msgs {
		content, err := proto.Marshal(&msg)
		if err != nil {
			pkg.LogError("Unexpected error: could not marshall message.")
			return err
		}
		p.send <- content
	}

	return nil
}

func ServeWs(hub *Hub, w http.ResponseWriter, r *http.Request) {
	conn, err := Upgrader.Upgrade(w, r, nil)
	if err != nil {
		pkg.LogError("Error attempting to build websocket connection: %s", err.Error())
		return
	}

	client := &Peer{hub: hub, Conn: conn, send: make(chan []byte, 256)}

	// Start read and write pumps
	go client.writePump()
	go client.handleConnectionPreparation()
}
