package sync

import (
	"JustSync/internal/config"
	"JustSync/pkg"

	y "github.com/skyterra/y-crdt"
)

type SyncService struct {
	config config.PeerConfig

	documents map[string]*y.Doc
}

func New(cfg config.PeerConfig) SyncService {
	return SyncService{
		config: cfg,

		documents: make(map[string]*y.Doc),
	}
}

func (s *SyncService) GetInitialSyncPayload() ([]byte, error) {
	pkg.LogInfo("Encoding state of %d documents for initial sync.", len(s.documents))

	payload, err := EncodeStateFromDisk(s.documents)
	if err != nil {
		pkg.LogError("Failed to encode initial sync payload: %v", err)
		return nil, err
	}

	return payload, nil
}
