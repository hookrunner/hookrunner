// previewd accepts ARM64 preview bundles and runs game servers inside bubblewrap.
package main

import (
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"
)

const maxUpload = 256 << 20

var shaPattern = regexp.MustCompile(`^[a-f0-9]{40}$`)
var hashPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)
var webHashPattern = regexp.MustCompile(`^[a-f0-9]{16}$`)

type config struct {
	listen, ip, token, repository, githubToken, data, bwrap string
	firstPort, lastPort, maxPreviews                        int
	ttl                                                     time.Duration
}

func env(name, fallback string) string {
	if value := os.Getenv(name); value != "" {
		return value
	}
	return fallback
}

func loadConfig() (config, error) {
	c := config{
		listen: env("PREVIEW_LISTEN", ":8080"), ip: os.Getenv("PREVIEW_IP"),
		token: os.Getenv("PREVIEW_TOKEN"), repository: env("PREVIEW_REPOSITORY", "hookrunner/hookrunner"),
		githubToken: os.Getenv("PREVIEW_GITHUB_TOKEN"), data: env("PREVIEW_DATA", "local/previews"),
	}
	if net.ParseIP(c.ip) == nil {
		return c, errors.New("PREVIEW_IP must be the VPS public IP address")
	}
	if len(c.token) < 32 {
		return c, errors.New("PREVIEW_TOKEN must contain at least 32 characters")
	}
	if !regexp.MustCompile(`^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$`).MatchString(c.repository) {
		return c, errors.New("PREVIEW_REPOSITORY must be owner/repository")
	}
	ports := strings.Split(env("PREVIEW_PORTS", "20000-20100"), "-")
	if len(ports) != 2 {
		return c, errors.New("PREVIEW_PORTS must be first-last")
	}
	var err error
	c.firstPort, err = strconv.Atoi(ports[0])
	if err != nil {
		return c, err
	}
	c.lastPort, err = strconv.Atoi(ports[1])
	if err != nil {
		return c, err
	}
	if c.firstPort < 1024 || c.lastPort > 65535 || c.lastPort < c.firstPort {
		return c, errors.New("PREVIEW_PORTS must be within 1024-65535")
	}
	c.maxPreviews, err = strconv.Atoi(env("PREVIEW_MAX", "5"))
	if err != nil || c.maxPreviews < 1 {
		return c, errors.New("invalid PREVIEW_MAX")
	}
	c.ttl, err = time.ParseDuration(env("PREVIEW_TTL", "24h"))
	if err != nil || c.ttl <= 0 {
		return c, errors.New("invalid PREVIEW_TTL")
	}
	c.data, err = filepath.Abs(c.data)
	if err != nil {
		return c, err
	}
	c.bwrap, err = exec.LookPath("bwrap")
	if err != nil {
		return c, errors.New("install bubblewrap: bwrap was not found")
	}
	return c, nil
}

type job struct {
	ID      string `json:"id"`
	PR      int    `json:"pr"`
	SHA     string `json:"sha"`
	Status  string `json:"status"`
	URL     string `json:"url,omitempty"`
	Error   string `json:"error,omitempty"`
	created time.Time
	archive string
	ctx     context.Context
	cancel  context.CancelFunc
}

type service struct {
	config  config
	mu      sync.Mutex
	jobs    map[string]*job
	current map[int]*job
	active  map[int]*preview
	queue   chan *job
	// Injectable only for local verification; production uses the GitHub API and bwrap.
	checkPR func(context.Context, int, string, bool) error
	launch  func(context.Context, string, config) (*preview, error)
}

func newService(c config) *service {
	s := &service{config: c, jobs: make(map[string]*job), current: make(map[int]*job), active: make(map[int]*preview), queue: make(chan *job, 16)}
	s.checkPR = s.verifyPR
	s.launch = launchPreview
	return s
}

func respond(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Cache-Control", "no-store")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}

func apiError(w http.ResponseWriter, status int, err error) {
	respond(w, status, map[string]string{"error": err.Error()})
}

func (s *service) handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("POST /deployments", s.upload)
	mux.HandleFunc("GET /deployments/{id}", func(w http.ResponseWriter, r *http.Request) {
		s.mu.Lock()
		defer s.mu.Unlock()
		j := s.jobs[r.PathValue("id")]
		if j == nil {
			apiError(w, 404, errors.New("deployment not found; retry upload if previewd restarted"))
			return
		}
		respond(w, 200, j)
	})
	mux.HandleFunc("DELETE /previews/{pr}", s.delete)
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if subtle.ConstantTimeCompare([]byte(r.Header.Get("Authorization")), []byte("Bearer "+s.config.token)) != 1 {
			apiError(w, http.StatusUnauthorized, errors.New("invalid deployment token"))
			return
		}
		mux.ServeHTTP(w, r)
	})
}

func parsePR(value string) (int, error) {
	pr, err := strconv.Atoi(value)
	if err != nil || pr < 1 || pr > 1_000_000_000 {
		return 0, errors.New("invalid PR number")
	}
	return pr, nil
}

func (s *service) upload(w http.ResponseWriter, r *http.Request) {
	pr, err := parsePR(r.URL.Query().Get("pr"))
	if err != nil {
		apiError(w, 400, err)
		return
	}
	sha := r.URL.Query().Get("sha")
	if !shaPattern.MatchString(sha) {
		apiError(w, 400, errors.New("sha must be a full lowercase commit SHA"))
		return
	}
	if err := s.checkPR(r.Context(), pr, sha, false); err != nil {
		apiError(w, 409, err)
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, maxUpload)
	archive, err := os.CreateTemp(s.config.data, "upload-*.tar.gz")
	if err != nil {
		apiError(w, 500, err)
		return
	}
	defer archive.Close()
	defer func() {
		if archive != nil {
			_ = os.Remove(archive.Name())
		}
	}()
	if _, err = io.Copy(archive, r.Body); err != nil {
		apiError(w, 413, err)
		return
	}
	if err = archive.Close(); err != nil {
		apiError(w, 500, err)
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	// Serialize the final head check with registration. Otherwise an older
	// upload could finish its check, pause, and then cancel a newer deployment.
	if err = s.checkPR(r.Context(), pr, sha, false); err != nil {
		apiError(w, 409, err)
		return
	}
	if previous := s.current[pr]; previous != nil && previous.SHA == sha && (previous.Status == "queued" || previous.Status == "deploying" || previous.Status == "ready") {
		respond(w, 202, previous)
		return
	}
	if _, exists := s.current[pr]; !exists && len(s.current) >= s.config.maxPreviews {
		apiError(w, 429, errors.New("active preview limit reached"))
		return
	}
	if len(s.queue) == cap(s.queue) {
		apiError(w, 429, errors.New("deployment queue is full"))
		return
	}
	id := make([]byte, 16)
	if _, err = rand.Read(id); err != nil {
		apiError(w, 500, err)
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	j := &job{ID: hex.EncodeToString(id), PR: pr, SHA: sha, Status: "queued", created: time.Now(), archive: archive.Name(), ctx: ctx, cancel: cancel}
	archive = nil // The worker now owns the temporary file.
	if previous := s.current[pr]; previous != nil {
		previous.cancel()
		if previous.Status == "queued" || previous.Status == "deploying" {
			previous.Status = "superseded"
		}
	}
	s.jobs[j.ID], s.current[pr] = j, j
	s.queue <- j
	respond(w, 202, j)
}

func (s *service) delete(w http.ResponseWriter, r *http.Request) {
	pr, err := parsePR(r.PathValue("pr"))
	if err != nil {
		apiError(w, 400, err)
		return
	}
	s.mu.Lock()
	// Serialize the closed check with deployment registration, too.
	if err = s.checkPR(r.Context(), pr, "", true); err != nil {
		s.mu.Unlock()
		apiError(w, 409, err)
		return
	}
	if j := s.current[pr]; j != nil {
		j.cancel()
		j.Status = "deleted"
		j.URL = ""
		delete(s.current, pr)
	}
	p := s.active[pr]
	delete(s.active, pr)
	s.mu.Unlock()
	if p != nil {
		p.stop()
	}
	respond(w, 200, map[string]string{"status": "deleted"})
}

func (s *service) worker(ctx context.Context) {
	for {
		select {
		case <-ctx.Done():
			return
		case j := <-s.queue:
			s.deploy(j)
		}
	}
}

func (s *service) deploy(j *job) {
	defer os.Remove(j.archive)
	s.mu.Lock()
	if j.ctx.Err() != nil {
		s.mu.Unlock()
		return
	}
	j.Status = "deploying"
	s.mu.Unlock()
	dir, err := os.MkdirTemp(s.config.data, "build-")
	if err == nil {
		err = unpack(j.ctx, j.archive, dir)
	}
	var p *preview
	if err == nil {
		p, err = s.launch(j.ctx, dir, s.config)
	}
	if err == nil {
		err = s.checkPR(j.ctx, j.PR, j.SHA, false)
	}
	s.mu.Lock()
	if j.ctx.Err() != nil || s.current[j.PR] != j {
		s.mu.Unlock()
		if p != nil {
			p.stop()
		}
		_ = os.RemoveAll(dir)
		return
	}
	if err != nil {
		j.Status, j.Error = "failed", err.Error()
		if s.active[j.PR] == nil {
			delete(s.current, j.PR)
		}
		s.mu.Unlock()
		if p != nil {
			p.stop()
		}
		_ = os.RemoveAll(dir)
		log.Printf("PR #%d deployment failed: %v", j.PR, err)
		return
	}
	old := s.active[j.PR]
	s.active[j.PR] = p
	s.mu.Unlock()
	// Stop the previous server and all of its WebSocket connections, not merely its HTTP listener.
	if old != nil {
		old.stop()
	}
	s.mu.Lock()
	if s.current[j.PR] == j && j.ctx.Err() == nil {
		j.Status, j.URL = "ready", p.url
	}
	s.mu.Unlock()
	log.Printf("PR #%d at %s (%s)", j.PR, p.url, j.SHA)
	go func() {
		<-p.done
		s.mu.Lock()
		if s.active[j.PR] == p {
			delete(s.active, j.PR)
			if s.current[j.PR] == j {
				delete(s.current, j.PR)
				j.Status = "failed"
				j.Error = "game server exited; retry deployment"
				j.URL = ""
			}
		}
		s.mu.Unlock()
		p.stop()
	}()
}

func (s *service) maintain(ctx context.Context) {
	ticker := time.NewTicker(time.Minute)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case now := <-ticker.C:
			s.mu.Lock()
			var expired []*preview
			for pr, p := range s.active {
				if now.Sub(p.created) > s.config.ttl {
					if j := s.current[pr]; j != nil && (j.Status == "queued" || j.Status == "deploying") {
						continue
					}
					if j := s.current[pr]; j != nil {
						j.cancel()
						j.Status = "expired"
						j.URL = ""
					}
					delete(s.current, pr)
					delete(s.active, pr)
					expired = append(expired, p)
				}
			}
			for id, j := range s.jobs {
				if s.current[j.PR] != j && now.Sub(j.created) > time.Hour {
					delete(s.jobs, id)
				}
			}
			s.mu.Unlock()
			for _, p := range expired {
				p.stop()
			}
		}
	}
}

func main() {
	c, err := loadConfig()
	if err != nil {
		log.Fatal(err)
	}
	if err = os.MkdirAll(c.data, 0700); err != nil {
		log.Fatal(err)
	}
	lock, err := os.OpenFile(filepath.Join(c.data, ".lock"), os.O_CREATE|os.O_RDWR, 0600)
	if err != nil {
		log.Fatal(err)
	}
	defer lock.Close()
	if err = syscall.Flock(int(lock.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		log.Fatal("another previewd is using PREVIEW_DATA")
	}
	// Previews are ephemeral. bwrap dies with its parent; a restart starts with no previews.
	for _, pattern := range []string{"build-*", "upload-*.tar.gz"} {
		paths, _ := filepath.Glob(filepath.Join(c.data, pattern))
		for _, path := range paths {
			if err = os.RemoveAll(path); err != nil {
				log.Fatal(err)
			}
		}
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	s := newService(c)
	var workers sync.WaitGroup
	workers.Add(2)
	go func() { defer workers.Done(); s.worker(ctx) }()
	go func() { defer workers.Done(); s.maintain(ctx) }()
	server := &http.Server{Addr: c.listen, Handler: s.handler(), ReadHeaderTimeout: 10 * time.Second, ReadTimeout: 5 * time.Minute, WriteTimeout: 5 * time.Minute, IdleTimeout: time.Minute, MaxHeaderBytes: 16 << 10}
	go func() {
		<-ctx.Done()
		s.mu.Lock()
		for _, j := range s.current {
			j.cancel()
		}
		s.mu.Unlock()
		_ = server.Close()
	}()
	log.Printf("preview API on %s; public IP %s, ports %d-%d", c.listen, c.ip, c.firstPort, c.lastPort)
	err = server.ListenAndServe()
	cancel()
	workers.Wait()
	s.mu.Lock()
	var previews []*preview
	for _, p := range s.active {
		previews = append(previews, p)
	}
	s.mu.Unlock()
	for _, p := range previews {
		p.stop()
	}
	if !errors.Is(err, http.ErrServerClosed) {
		log.Fatal(err)
	}
}
