package main

import (
	"JustSync/api"
	"JustSync/utils"
	"bufio"
	"flag"
	"fmt"
	"log/slog"
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
	utils.LogDebug("Debug log")

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
	http.HandleFunc("/setup", api.Setup)
	http.HandleFunc("/send-sync", api.RequestSync)
	if err := http.ListenAndServe(":10000", nil); err != nil {
		slog.Error(err.Error())
	}
}

func runClientMode() {
	//TODO:
}

func runAdminMode() {
	slog.Info("Admin console")
	slog.Info("Commands: new-otp, exit")

	tm := utils.NewTokenManager()
	reader := bufio.NewReader(os.Stdin)

	for {
		fmt.Print("> ")
		input, _ := reader.ReadString('\n')
		input = strings.TrimSpace(input)

		switch input {
		case "new-otp":
			otp := tm.GenerateOtp()
			slog.Info("Generated otp: %s\n", otp)
			slog.Info("Generated otp expires in %.0f minutes\n", utils.OtpExpiration.Minutes())
		case "exit":
			os.Exit(0)
		default:
			slog.Error("Unknown command.")
		}
	}
}
