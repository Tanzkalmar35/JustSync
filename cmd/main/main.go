package main

import (
	"JustSync/api"
	"JustSync/service"
	"JustSync/utils"
	socket "JustSync/websocket"
	"bufio"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"

	"github.com/gorilla/websocket"
)

func main() {
	var mode utils.RunMode = utils.ServerMode

	// Logger initialization - Set debug mode
	utils.SetLevel(utils.LevelInfo)

	// Capture run mode from cmd line args
	flag.Var(&mode, "mode", "Run mode: server, client, admin")
	cfgName := flag.String("config", "", "The config to load up")
	flag.Parse()

	// Set global runtime mode
	utils.SetMode(mode)

	utils.LogInfo("Starting application in %s mode", mode.String())

	// Start logic loop
	switch mode {
	case utils.ServerMode:
		runServerMode(*cfgName)
	case utils.ClientMode:
		runClientMode(*cfgName)
	case utils.AdminMode:
		runAdminMode()
	}
}

func runServerMode(cfg string) {
	utils.CreateConfigFolderAt(utils.GetOsSpecificConfigPath())
	config := utils.InitHostConfig(cfg)

	http.HandleFunc("/connect", api.HandleConnectClient)
	http.HandleFunc("/admin/generateOtp", api.HandleGenerateOtp)

	utils.LogInfo("Server running at port %s", config.Application.Port)

	if err := http.ListenAndServe(config.Application.Port, nil); err != nil {
		utils.LogError(err.Error())
	}
}

func runClientMode(cfgName string) {
	cfg := utils.InitClientConfig(cfgName)
	host := "wss://" + cfg.Session.Host.Url + "/connect"
	utils.LogInfo("Attempting to connect to: %s", host)

	conn, _, err := websocket.DefaultDialer.Dial(host, nil)
	if err != nil {
		utils.LogError("Could not dial %s due to error: %s", host, err.Error())
		return
	}
	defer conn.Close()

	utils.LogInfo("Connection to host at %s established successfully", host)
	utils.LogInfo("Attempting authentication handshake")

	err = conn.WriteMessage(websocket.TextMessage, []byte(cfg.Session.Client.Token))
	if err != nil {
		utils.LogError("Authentication token for handshake could not be sent: %s", err.Error())
		return
	}

	socket.SetHostConnection(conn)

	http.HandleFunc("/send-sync", api.RequestSync)
	go service.KeepClientAlive(conn)
	go service.HandleReceiveAndProcessIncomingMessages(conn)

	utils.LogInfo("Listening for sync requests on localhost port :10001")

	if err := http.ListenAndServe(cfg.Session.Port, nil); err != nil {
		utils.LogError(err.Error())
	}

	utils.LogWarn("Connection to host has been lost. Shutting down.")
}

func runAdminMode() {
	utils.LogInfo("Admin console")
	utils.LogInfo("Commands: new-otp, exit")

	reader := bufio.NewReader(os.Stdin)

	for {
		fmt.Print("> ")
		input, _ := reader.ReadString('\n')
		input = strings.TrimSpace(input)

		switch input {
		case "new-otp":
			resp, err := http.Get("http://localhost:10000/admin/generateOtp?t=SECRETKEY")
			if err != nil {
				utils.LogError("Error retrieving otp, is the server running?")
				os.Exit(0)
			}
			defer resp.Body.Close()

			body, err := io.ReadAll(resp.Body)
			if err != nil {
				utils.LogError("Something went wrong while generating otp: %s", err.Error())
				os.Exit(0)
			}

			utils.LogInfo("Generated otp: %s", string(body))
			utils.LogInfo("Generated otp expires in %.0f minutes", utils.OtpExpiration.Minutes())
		case "exit":
			os.Exit(0)
		default:
			utils.LogError("Unknown command.")
		}
	}
}
