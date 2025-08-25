package sync

import (
	y "github.com/skyterra/y-crdt"
)

func CreateDocFromContent(relativePath string, content []byte) *y.Doc {
	doc := y.NewDoc(relativePath, false, nil, nil, false)
	text := doc.GetText("content")
	text.Insert(0, string(content), nil)
	return doc
}

func EncodeStateFromDisk(docs map[string]*y.Doc) ([]byte, error) {
	var individualUpdates [][]byte

	for _, doc := range docs {
		update := y.EncodeStateAsUpdateV2(doc, nil, nil)
		individualUpdates = append(individualUpdates, update)
	}

	mergedPayload := y.MergeUpdatesV2(individualUpdates, y.NewUpdateDecoderV1, y.NewUpdateEncoderV1, true)
	return mergedPayload, nil
}
