package main

import (
	"archive/tar"
	"bufio"
	"compress/gzip"
	"context"
	"debug/elf"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httputil"
	"net/url"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"
)

// A bundle contains only a server, the browser build and the simulation hash.
// Reject links, special files, traversal and decompression bombs before launch.
func unpack(ctx context.Context, archive, destination string) error {
	f, err := os.Open(archive)
	if err != nil {
		return err
	}
	defer f.Close()
	z, err := gzip.NewReader(f)
	if err != nil {
		return err
	}
	defer z.Close()
	t := tar.NewReader(z)
	var size int64
	count := 0
	for {
		if err = ctx.Err(); err != nil {
			return err
		}
		h, nextErr := t.Next()
		if nextErr == io.EOF {
			break
		}
		if nextErr != nil {
			return nextErr
		}
		name := strings.TrimPrefix(h.Name, "./")
		name = strings.TrimSuffix(name, "/")
		if name == "." && h.Typeflag == tar.TypeDir {
			continue
		}
		if name == "" || path.Clean(name) != name || !filepath.IsLocal(name) || strings.Contains(name, "\\") {
			return fmt.Errorf("invalid archive path %q", h.Name)
		}
		if name != "hookrunner-server" && name != "simulation-build.txt" && name != "dist" && !strings.HasPrefix(name, "dist/") {
			return fmt.Errorf("unexpected bundle file %q", name)
		}
		count++
		if count > 10000 || h.Size < 0 || h.Size > (512<<20)-size {
			return errors.New("expanded bundle exceeds limits")
		}
		size += h.Size
		target := filepath.Join(destination, name)
		if h.Typeflag == tar.TypeDir {
			if name == "hookrunner-server" || name == "simulation-build.txt" {
				return errors.New("server and simulation hash must be regular files")
			}
			if err = os.MkdirAll(target, 0700); err != nil {
				return err
			}
			continue
		}
		if h.Typeflag != tar.TypeReg {
			return errors.New("bundle may contain only regular files and directories")
		}
		if err = os.MkdirAll(filepath.Dir(target), 0700); err != nil {
			return err
		}
		mode := os.FileMode(0600)
		if name == "hookrunner-server" {
			mode = 0700
		}
		out, err := os.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, mode)
		if err != nil {
			return err
		}
		_, copyErr := io.Copy(out, t)
		closeErr := out.Close()
		if copyErr != nil {
			return copyErr
		}
		if closeErr != nil {
			return closeErr
		}
	}
	for _, name := range []string{"hookrunner-server", "simulation-build.txt", "dist/index.html", "dist/build.json"} {
		info, err := os.Stat(filepath.Join(destination, name))
		if err != nil || !info.Mode().IsRegular() {
			return fmt.Errorf("bundle is missing %s", name)
		}
	}
	// Do not execute an uploaded binary to discover its architecture or build ID.
	binary, err := elf.Open(filepath.Join(destination, "hookrunner-server"))
	if err != nil {
		return err
	}
	defer binary.Close()
	if binary.Machine != elf.EM_AARCH64 || binary.Class != elf.ELFCLASS64 {
		return errors.New("game server must be a Linux ARM64 ELF binary")
	}
	build, err := os.ReadFile(filepath.Join(destination, "simulation-build.txt"))
	if err != nil {
		return err
	}
	if !hashPattern.MatchString(strings.TrimSpace(string(build))) {
		return errors.New("invalid simulation build hash")
	}
	return nil
}

type preview struct {
	url, directory string
	created        time.Time
	cmd            *exec.Cmd
	done           chan struct{}
	web            *http.Server
	handler        http.Handler
	once           sync.Once
}

func (p *preview) stop() {
	p.once.Do(func() {
		if p.web != nil {
			_ = p.web.Close()
		}
		if p.cmd != nil && p.cmd.Process != nil {
			// Killing bwrap also kills its isolated PID namespace and descendants.
			_ = p.cmd.Process.Kill()
			<-p.done
		}
		_ = os.RemoveAll(p.directory)
	})
}

func sandboxArgs(binary, address string) []string {
	args := []string{"--unshare-all", "--unshare-user", "--share-net", "--die-with-parent", "--new-session", "--cap-drop", "ALL", "--clearenv", "--ro-bind", "/usr", "/usr"}
	for _, library := range []string{"/lib", "/lib64"} {
		if _, err := os.Stat(library); err == nil {
			args = append(args, "--ro-bind", library, library)
		}
	}
	if _, err := os.Stat("/etc/ld.so.cache"); err == nil {
		args = append(args, "--ro-bind", "/etc/ld.so.cache", "/etc/ld.so.cache")
	}
	return append(args, "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--dir", "/app", "--ro-bind", binary, "/app/server", "--chdir", "/app", "--setenv", "HOOKRUNNER_BIND", address, "--setenv", "RUST_LOG", "info", "--", "/app/server")
}

type cappedLog struct {
	mu        sync.Mutex
	file      *os.File
	remaining int
}

func (w *cappedLog) Write(data []byte) (int, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	n := len(data)
	if len(data) > w.remaining {
		data = data[:w.remaining]
	}
	if len(data) != 0 {
		if _, err := w.file.Write(data); err != nil {
			return 0, err
		}
		w.remaining -= len(data)
	}
	return n, nil
}

func listenPreview(c config) (net.Listener, error) {
	for port := c.firstPort; port <= c.lastPort; port++ {
		listener, err := net.Listen("tcp", net.JoinHostPort("", strconv.Itoa(port)))
		if err == nil {
			return listener, nil
		}
	}
	return nil, errors.New("no free preview HTTP port")
}

func launchPreview(ctx context.Context, dir string, c config) (*preview, error) {
	var public net.Listener
	var err error
	if !c.mainDeployment {
		public, err = listenPreview(c)
		if err != nil {
			return nil, err
		}
	}
	defer func() {
		if public != nil {
			_ = public.Close()
		}
	}()
	build, err := os.ReadFile(filepath.Join(dir, "simulation-build.txt"))
	if err != nil {
		return nil, err
	}
	var manifest struct {
		Build string `json:"build"`
	}
	manifestFile, err := os.ReadFile(filepath.Join(dir, "dist/build.json"))
	if err != nil {
		return nil, err
	}
	if err = json.Unmarshal(manifestFile, &manifest); err != nil || !webHashPattern.MatchString(manifest.Build) {
		return nil, errors.New("invalid browser build manifest")
	}
	// Hold the public listener throughout deployment. For the Rust listener,
	// reserve a loopback port then release it just before launch; retry collisions.
	for attempt := 0; attempt < 3; attempt++ {
		if err = ctx.Err(); err != nil {
			return nil, err
		}
		reservation, err := net.Listen("tcp", "127.0.0.1:0")
		if err != nil {
			return nil, err
		}
		address := reservation.Addr().String()
		p := &preview{directory: dir, created: time.Now(), done: make(chan struct{})}
		logFile, err := os.OpenFile(filepath.Join(dir, "server.log"), os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0600)
		if err != nil {
			reservation.Close()
			return nil, err
		}
		output := &cappedLog{file: logFile, remaining: 1 << 20}
		p.cmd = exec.Command(c.bwrap, sandboxArgs(filepath.Join(dir, "hookrunner-server"), address)...)
		p.cmd.Env = []string{"PATH=/usr/bin:/bin"}
		p.cmd.Stdout, p.cmd.Stderr = output, output
		started := make(chan error, 1)
		go func() {
			// bwrap's parent-death signal is tied to its spawning OS thread.
			// Keep that thread alive for the entire game process lifetime.
			runtime.LockOSThread()
			defer runtime.UnlockOSThread()
			_ = reservation.Close()
			err := p.cmd.Start()
			started <- err
			if err == nil {
				_ = p.cmd.Wait()
			}
			_ = logFile.Close()
			close(p.done)
		}()
		if err = <-started; err != nil {
			<-p.done
			return nil, err
		}
		err = waitReady(ctx, address, strings.TrimSpace(string(build)), p.done)
		if err != nil {
			_ = p.cmd.Process.Kill()
			<-p.done
			logs, _ := os.ReadFile(filepath.Join(dir, "server.log"))
			if strings.Contains(strings.ToLower(string(logs)), "address already in use") || strings.Contains(string(logs), "os error 98") {
				continue
			}
			if len(logs) > 4096 {
				logs = logs[len(logs)-4096:]
			}
			return nil, fmt.Errorf("%w: %s", err, strings.TrimSpace(string(logs)))
		}
		port := c.mainPort
		if public != nil {
			port = public.Addr().(*net.TCPAddr).Port
		}
		endpoint := &url.URL{Scheme: "http", Host: net.JoinHostPort(c.ip, strconv.Itoa(port)), Path: "/"}
		p.url = endpoint.String()
		p.handler = gameHandler(filepath.Join(dir, "dist"), address, manifest.Build)
		if public != nil {
			p.web = &http.Server{Handler: p.handler, ReadHeaderTimeout: 10 * time.Second, IdleTimeout: time.Minute, MaxHeaderBytes: 16 << 10}
			listener := public
			public = nil
			go func() { _ = p.web.Serve(listener) }()
		}
		return p, nil
	}
	return nil, errors.New("could not reserve a game server port after three attempts")
}

func waitReady(ctx context.Context, address, build string, done <-chan struct{}) error {
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return fmt.Errorf("game readiness: %w", ctx.Err())
		case <-done:
			return errors.New("game server exited before becoming ready")
		case <-ticker.C:
			conn, err := (&net.Dialer{Timeout: time.Second}).DialContext(ctx, "tcp", address)
			if err != nil {
				continue
			}
			_ = conn.SetDeadline(time.Now().Add(time.Second))
			_, err = fmt.Fprintf(conn, "GET /?build=%s HTTP/1.1\r\nHost: %s\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n", build, address)
			if err == nil {
				response, err := http.ReadResponse(bufio.NewReader(conn), &http.Request{Method: "GET"})
				if err == nil && response.StatusCode == 101 && response.Header.Get("Sec-WebSocket-Accept") == "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=" {
					conn.Close()
					return nil
				}
			}
			_ = conn.Close()
		}
	}
}

func gameHandler(directory, address, build string) http.Handler {
	target := &url.URL{Scheme: "http", Host: address}
	proxy := &httputil.ReverseProxy{Rewrite: func(r *httputil.ProxyRequest) {
		r.SetURL(target)
		r.Out.URL.Path = "/"
		r.Out.URL.RawPath = ""
		r.SetXForwarded()
	}}
	files := http.FileServer(http.Dir(directory))
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/ws" {
			proxy.ServeHTTP(w, r)
			return
		}
		if r.Method != "GET" && r.Method != "HEAD" {
			w.WriteHeader(405)
			return
		}
		entry := r.URL.Path == "/" || r.URL.Path == "/index.html" || r.URL.Path == "/build.json"
		if entry {
			w.Header().Set("Cache-Control", "no-store")
			r.Header.Del("If-Modified-Since")
			r.Header.Del("If-None-Match")
		} else {
			w.Header().Set("Cache-Control", "no-cache")
		}
		requested := r.Header.Get("X-Hookrunner-Build")
		parts := strings.Split(r.URL.Path, "/")
		if len(parts) >= 3 && parts[1] == "pkg" {
			requested = parts[2]
		}
		if requested != "" && requested != build {
			w.Header().Set("Clear-Site-Data", `"cache"`)
		}
		files.ServeHTTP(w, r)
	})
}
