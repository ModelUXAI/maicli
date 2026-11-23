package maicli

import (
	"bytes"
	"strings"
	"testing"
)

func TestPromptRoundtrip(t *testing.T) {
	masukan := strings.Join([]string{
		`{"type":"ready","protocol":"maicli-rpc/1","model":"mock"}`,
		`{"type":"event","event":{"event":"agent_start"}}`,
		`{"type":"prompt_result","id":"go-1","status":"ok"}`,
		`{"type":"result","id":"go-1","stop_reason":"stop","turns":1}`,
		"",
	}, "\n")
	keluar := &bytes.Buffer{}
	c := New(strings.NewReader(masukan), keluar)
	var events int
	c.OnEvent = func(Frame) { events++ }
	f, err := c.Prompt("halo")
	if err != nil {
		t.Fatal(err)
	}
	if f.StopReason != "stop" {
		t.Fatalf("stop reason = %q", f.StopReason)
	}
	if events != 1 {
		t.Fatalf("events = %d", events)
	}
	if !strings.Contains(keluar.String(), `"message":"halo"`) {
		t.Fatalf("permintaan tidak terkirim: %s", keluar.String())
	}
}
