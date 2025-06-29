package main

import (
	"JustSync/api"
	"JustSync/utils"
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"net/http"
	"os"
	"strings"
)

func main() {
	var mode utils.RunMode = utils.ServerMode

	// Logger initialization - Set debug mode
	utils.SetLevel(utils.LevelInfo)

	// Capture run mode from cmd line args
	flag.Var(&mode, "mode", "Run mode: server, client, admin")
	flag.Parse()

	// Set global runtime mode
	utils.SetMode(mode)

	utils.LogInfo("Starting application in %s mode", mode.String())

	// Start logic loop
	switch mode {
	case utils.ServerMode:
		runServerMode()
	case utils.ClientMode:
		runClientMode()
	case utils.AdminMode:
		runAdminMode()
	}
}

func runServerMode() {
	port := ":10000"
	utils.CreateConfigFolderAt(utils.GetOsSpecificConfigPath())

	http.HandleFunc("/setup", api.Setup)
	http.HandleFunc("/send-sync", api.RequestSync)
	http.HandleFunc("/authenticate", api.AuthenticateClient)
	http.HandleFunc("/admin/generateOtp", api.HandleGenerateOtp)

	utils.LogInfo("Server running at port %s", port)

	if err := http.ListenAndServe(port, nil); err != nil {
		utils.LogError(err.Error())
	}
}

func runClientMode() {
	utils.CreateConfigFolderAt(utils.GetOsSpecificConfigPath())
	// TODO:
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
			var otpReq struct{ otp string }

			req, err := http.Get("localhost:10000/admin/generateOtp?t=SECRETKEY")
			if err != nil {
				utils.LogError("Error retrieving otp, is the server running?")
			}

			if err := json.NewDecoder(req.Body).Decode(&otpReq); err != nil {
				utils.LogError("Error retrieving otp: %s", err.Error())
			}

			utils.LogInfo("Generated otp: %s\n", otpReq.otp)
			utils.LogInfo("Generated otp expires in %.0f minutes\n", utils.OtpExpiration.Minutes())
		case "exit":
			os.Exit(0)
		default:
			utils.LogError("Unknown command.")
		}
	}
}
