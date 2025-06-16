package api

import (
	"JustSync/entities"
	"JustSync/service"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
)

// Accepts json data
func setup(w http.ResponseWriter, r *http.Request) {
	fmt.Println("Setup request recieved")

	var body entities.SyncRequest
	err := json.NewDecoder(r.Body).Decode(&body)

	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}

	err = service.HandleCreateSnapshot(body.Path)

	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}

	fmt.Println("Setup request accepted")
}

func HandleRequests() {
	http.HandleFunc("/setup", setup)
	log.Fatal(http.ListenAndServe(":10000", nil))
}
