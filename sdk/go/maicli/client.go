// Package maicli — klien RPC maicli v1 (JSONL di stdin/stdout), tanpa dependensi eksternal.
package maicli

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"sync"
)

// Frame balasan maicli (field tambahan diabaikan).
type Frame struct {
	Type       string          `json:"type"`
	ID         string          `json:"id,omitempty"`
	Result     json.RawMessage `json:"result,omitempty"`
	Error      json.RawMessage `json:"error,omitempty"`
	Question   string          `json:"question,omitempty"`
	Tool       string          `json:"tool,omitempty"`
	Tier       string          `json:"tier,omitempty"`
	Subject    string          `json:"subject,omitempty"`
	StopReason string          `json:"stop_reason,omitempty"`
}

// Client memakai reader/writer apa pun (proses, pipe, buffer test).
type Client struct {
	in     *bufio.Reader
	out    io.Writer
	mu     sync.Mutex
	nextID int
	// OnQuestion/OnApproval dipanggil untuk frame interaktif; balasan dikirim otomatis.
	OnQuestion  func(id, question string) string
	OnApproval  func(id, tool, tier, subject string) string
	OnEvent     func(frame Frame)
}

// New bangun klien.
func New(r io.Reader, w io.Writer) *Client { return &Client{in: bufio.NewReader(r), out: w} }

func (c *Client) kirim(v any) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	b, err := json.Marshal(v)
	if err != nil {
		return err
	}
	_, err = fmt.Fprintln(c.out, string(b))
	return err
}

// Prompt jalankan satu turn; mengembalikan frame result terakhir.
func (c *Client) Prompt(pesan string) (Frame, error) {
	c.nextID++
	id := fmt.Sprintf("go-%d", c.nextID)
	if err := c.kirim(map[string]any{"type": "prompt", "id": id, "message": pesan}); err != nil {
		return Frame{}, err
	}
	for {
		baris, err := c.in.ReadString('\n')
		if err != nil {
			return Frame{}, err
		}
		var f Frame
		if err := json.Unmarshal([]byte(baris), &f); err != nil {
			return Frame{}, err
		}
		switch f.Type {
		case "question":
			jawaban := ""
			if c.OnQuestion != nil {
				jawaban = c.OnQuestion(f.ID, f.Question)
			}
			_ = c.kirim(map[string]any{"type": "answer", "id": f.ID, "text": jawaban})
		case "approval_request":
			keputusan := "deny"
			if c.OnApproval != nil {
				keputusan = c.OnApproval(f.ID, f.Tool, f.Tier, f.Subject)
			}
			_ = c.kirim(map[string]any{"type": "approval", "id": f.ID, "decision": keputusan})
		case "event":
			if c.OnEvent != nil {
				c.OnEvent(f)
			}
		case "result":
			if f.ID == id {
				return f, nil
			}
		case "error":
			if f.ID == id {
				return f, fmt.Errorf("maicli: %s", string(f.Error))
			}
		}
	}
}

// Ping kirim frame ping; balasan pong dipetakan ke Frame.
func (c *Client) Ping() (Frame, error) {
	c.nextID++
	id := fmt.Sprintf("go-%d", c.nextID)
	if err := c.kirim(map[string]any{"type": "ping", "id": id}); err != nil {
		return Frame{}, err
	}
	for {
		baris, err := c.in.ReadString('\n')
		if err != nil {
			return Frame{}, err
		}
		var f Frame
		if err := json.Unmarshal([]byte(baris), &f); err != nil {
			return Frame{}, err
		}
		if f.Type == "pong" && f.ID == id {
			return f, nil
		}
	}
}

// Shutdown menutup server dengan rapi.
func (c *Client) Shutdown() error { return c.kirim(map[string]any{"type": "shutdown"}) }
