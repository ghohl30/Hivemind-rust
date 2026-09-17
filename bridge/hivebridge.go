// Command hivebridge plays hive-engine (Rust) against hiveGo's AI and reports
// the score.
//
// It is built inside a checkout of github.com/janpfeifer/hiveGo, because the
// packages it needs (internal/state, internal/ai/...) are under internal/ and
// so are importable only from within that module. See bridge/README.md; the
// setup script copies this file to cmd/hivebridge/main.go there.
//
// hiveGo's Board is the sole arbiter of legality and of the result. The Rust
// engine is a subprocess speaking the line protocol in engine/examples/bridge.rs;
// every move by either side is fed to it with "apply", and its own moves come
// back from "go".
//
// Deliberately NOT importing internal/players/default: that package registers
// the GoMLX-backed scorers and pulls the whole XLA stack into the build. We
// register just the linear scorer and the two searchers by hand, which is all
// an -tags-free "linear,ab,..." config needs.
package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"math/rand"
	"os"
	"os/exec"
	"sort"
	"strings"
	"time"

	"github.com/janpfeifer/hiveGo/internal/ai/linear"
	"github.com/janpfeifer/hiveGo/internal/players"
	"github.com/janpfeifer/hiveGo/internal/searchers/alphabeta"
	"github.com/janpfeifer/hiveGo/internal/searchers/mcts"
	"github.com/janpfeifer/hiveGo/internal/state"
)

var (
	flagRust     = flag.String("rust", "", "Path to the Rust bridge binary (target/release/examples/bridge).")
	flagAI       = flag.String("ai", "linear,ab,max_time=100ms", "hiveGo AI config string.")
	flagThink    = flag.Duration("think", 100*time.Millisecond, "Think time budget handed to the Rust engine per move.")
	flagGames    = flag.Int("games", 20, "Number of games. Colours swap every game, so use an even number.")
	flagMaxMoves = flag.Int("max_moves", state.DefaultMaxMoves, "Plies before the game is scored a draw.")
	flagOpening  = flag.Int("opening", 4, "Random plies played by both sides before the engines take over.")
	flagSeed     = flag.Int64("seed", 1, "RNG seed for the random openings.")
	flagValidate = flag.Bool("validate", true, "Cross-check the two move generators at every ply.")
	flagVerbose  = flag.Bool("verbose", false, "Print every move.")
	flagFuzz     = flag.Int("fuzz", 0, "Instead of playing, run N random playouts validating both move generators at every ply. Deterministic given -seed, and far faster than a match because no search runs.")
)

// ---------------------------------------------------------------------------
// Move tokens
// ---------------------------------------------------------------------------

// encodeAction renders an Action in the wire format understood by the Rust
// bridge. Placements carry a piece letter rather than an identity, which is
// what lets the two engines agree despite hive-engine tracking individual
// PieceIds and hiveGo tracking only counts per type.
func encodeAction(a state.Action) string {
	if a.IsSkipAction() {
		return "X"
	}
	if a.Move {
		return fmt.Sprintf("M%d,%d,%d,%d", a.SourcePos[0], a.SourcePos[1], a.TargetPos[0], a.TargetPos[1])
	}
	return fmt.Sprintf("P%s%d,%d", state.PieceLetters[a.Piece], a.TargetPos[0], a.TargetPos[1])
}

// decodeAction finds the legal action matching a token. Resolving through the
// board's own action list (rather than parsing into an Action directly) means
// an unparseable or illegal token fails loudly here instead of producing a
// plausible-looking but illegal move.
func decodeAction(b *state.Board, tok string) (state.Action, error) {
	for _, a := range b.Derived.Actions {
		if encodeAction(a) == tok {
			return a, nil
		}
	}
	return state.Action{}, fmt.Errorf("token %q is not a legal action here", tok)
}

// actionTokens is the deduplicated, sorted token set for the side to move --
// the form the Rust engine's "moves" reply also takes, so the two are directly
// comparable.
func actionTokens(b *state.Board) []string {
	seen := map[string]bool{}
	var toks []string
	for _, a := range b.Derived.Actions {
		t := encodeAction(a)
		if !seen[t] {
			seen[t] = true
			toks = append(toks, t)
		}
	}
	sort.Strings(toks)
	return toks
}

// history of tokens played in the current game, so a validation failure can be
// replayed exactly rather than hunted for again under a different clock.
var history []string

// dumpBoard renders every piece with its height, which is what beetle
// disagreements actually turn on.
func dumpBoard(b *state.Board) string {
	var sb strings.Builder
	type entry struct {
		pos     state.Pos
		player  state.PlayerNum
		piece   state.PieceType
		covered bool
	}
	var es []entry
	b.EnumeratePieces(func(player state.PlayerNum, piece state.PieceType, pos state.Pos, covered bool) {
		es = append(es, entry{pos, player, piece, covered})
	})
	sort.Slice(es, func(i, j int) bool {
		if es[i].pos[0] != es[j].pos[0] {
			return es[i].pos[0] < es[j].pos[0]
		}
		return es[i].pos[1] < es[j].pos[1]
	})
	for _, e := range es {
		sb.WriteString(fmt.Sprintf("    (%d,%d) player=%d %s stackHeight=%d covered=%v\n",
			e.pos[0], e.pos[1], e.player, state.PieceLetters[e.piece], b.CountAt(e.pos), e.covered))
	}
	return sb.String()
}

// ---------------------------------------------------------------------------
// Rust engine subprocess
// ---------------------------------------------------------------------------

type rustEngine struct {
	cmd *exec.Cmd
	in  io.WriteCloser
	out *bufio.Reader
}

func startRust(path string) (*rustEngine, error) {
	cmd := exec.Command(path)
	cmd.Stderr = os.Stderr
	in, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	out, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	return &rustEngine{cmd: cmd, in: in, out: bufio.NewReader(out)}, nil
}

func (r *rustEngine) send(format string, args ...any) (string, error) {
	if _, err := fmt.Fprintf(r.in, format+"\n", args...); err != nil {
		return "", fmt.Errorf("writing to Rust engine: %w", err)
	}
	line, err := r.out.ReadString('\n')
	if err != nil {
		return "", fmt.Errorf("reading from Rust engine: %w", err)
	}
	line = strings.TrimSpace(line)
	if strings.HasPrefix(line, "error") {
		return "", fmt.Errorf("Rust engine: %s", line)
	}
	return line, nil
}

func (r *rustEngine) close() {
	_, _ = fmt.Fprintln(r.in, "quit")
	_ = r.in.Close()
	_ = r.cmd.Wait()
}

// ---------------------------------------------------------------------------
// Match play
// ---------------------------------------------------------------------------

type tally struct {
	rustWins, goWins, draws int
	// Split by seat, because Hive has a real first-player advantage and a
	// score that ignores it can be mostly seat luck.
	rustWinsAsFirst, rustWinsAsSecond int
	drawReasons                       map[string]int
	rustNodes, rustDepthSum, rustMoves int64
}

// playGame plays one game and returns the winner from the Rust engine's point
// of view: +1 Rust, -1 hiveGo, 0 draw.
func playGame(r *rustEngine, goAI *players.SearcherScorer, rustIsFirst bool, rng *rand.Rand, t *tally) (int, error) {
	b := state.NewBoard()
	b.MaxMoves = *flagMaxMoves
	if _, err := r.send("init"); err != nil {
		return 0, err
	}

	rustPlayer := state.PlayerFirst
	if !rustIsFirst {
		rustPlayer = state.PlayerSecond
	}

	history = nil
	apply := func(a state.Action) error {
		tok := encodeAction(a)
		if _, err := r.send("apply %s", tok); err != nil {
			return err
		}
		history = append(history, tok)
		b = b.Act(a)
		return nil
	}

	// Randomised opening. Both engines are deterministic, so without this
	// every game in a run would be the same game -- the trap examples/match.rs
	// fell into.
	for ply := 0; ply < *flagOpening && !b.IsFinished(); ply++ {
		acts := b.Derived.Actions
		if err := apply(acts[rng.Intn(len(acts))]); err != nil {
			return 0, err
		}
	}

	for !b.IsFinished() {
		if *flagValidate {
			if err := validate(r, b); err != nil {
				return 0, err
			}
		}

		var a state.Action
		if b.NextPlayer == rustPlayer {
			line, err := r.send("go %d", flagThink.Milliseconds())
			if err != nil {
				return 0, err
			}
			// "bestmove <tok> depth <d> nodes <n>"
			f := strings.Fields(line)
			if len(f) < 2 || f[0] != "bestmove" {
				return 0, fmt.Errorf("unexpected reply to go: %q", line)
			}
			a, err = decodeAction(b, f[1])
			if err != nil {
				return 0, fmt.Errorf("Rust engine returned an illegal move: %w", err)
			}
			if len(f) >= 6 {
				var d, n int64
				fmt.Sscanf(f[3], "%d", &d)
				fmt.Sscanf(f[5], "%d", &n)
				t.rustDepthSum += d
				t.rustNodes += n
				t.rustMoves++
			}
		} else {
			a, _, _, _ = goAI.Play(b)
		}
		if *flagVerbose {
			who := "hiveGo"
			if b.NextPlayer == rustPlayer {
				who = "rust  "
			}
			fmt.Printf("  ply %3d %s %s\n", b.MoveNumber, who, a)
		}
		if err := apply(a); err != nil {
			return 0, err
		}
	}

	if t.drawReasons == nil {
		t.drawReasons = map[string]int{}
	}
	if b.Draw() {
		t.drawReasons[b.FinishReason()]++
		return 0, nil
	}
	if b.Winner() == rustPlayer {
		return 1, nil
	}
	return -1, nil
}

// validate compares the two move generators for the side to move. A
// disagreement means the match is measuring a rules mismatch rather than
// playing strength, so it is fatal rather than a warning.
func validate(r *rustEngine, b *state.Board) error {
	line, err := r.send("moves")
	if err != nil {
		return err
	}
	rustToks := strings.Fields(strings.TrimPrefix(line, "moves"))
	goToks := actionTokens(b)
	sort.Strings(rustToks)
	if strings.Join(rustToks, " ") == strings.Join(goToks, " ") {
		return nil
	}
	return fmt.Errorf("move generators disagree at ply %d (side %v)\n  only-in-hiveGo: %v\n  only-in-rust:   %v\n  board:\n%s  history: %s",
		b.MoveNumber, b.NextPlayer,
		difference(goToks, rustToks), difference(rustToks, goToks),
		dumpBoard(b), strings.Join(history, " "))
}

func difference(a, b []string) []string {
	inB := map[string]bool{}
	for _, s := range b {
		inB[s] = true
	}
	var out []string
	for _, s := range a {
		if !inB[s] {
			out = append(out, s)
		}
	}
	return out
}

// fuzz plays random legal games, checking both move generators at every ply.
// No search runs, so it is fully reproducible from -seed and fast enough to
// cover far more positions than a match ever would.
func fuzz(r *rustEngine, rng *rand.Rand, games int) error {
	plies := 0
	for g := 0; g < games; g++ {
		b := state.NewBoard()
		b.MaxMoves = *flagMaxMoves
		if _, err := r.send("init"); err != nil {
			return err
		}
		history = nil
		for !b.IsFinished() {
			if err := validate(r, b); err != nil {
				return fmt.Errorf("playout %d (seed %d): %w", g+1, *flagSeed, err)
			}
			a := b.Derived.Actions[rng.Intn(len(b.Derived.Actions))]
			tok := encodeAction(a)
			if _, err := r.send("apply %s", tok); err != nil {
				return err
			}
			history = append(history, tok)
			b = b.Act(a)
			plies++
		}
		if (g+1)%50 == 0 {
			fmt.Printf("  fuzz: %d playouts, %d plies checked\n", g+1, plies)
		}
	}
	fmt.Printf("  fuzz: %d plies checked\n", plies)
	return nil
}

func main() {
	flag.Parse()
	if *flagRust == "" {
		fmt.Fprintln(os.Stderr, "-rust is required (path to target/release/examples/bridge)")
		os.Exit(2)
	}

	players.RegisteredScorers = append(players.RegisteredScorers, linear.NewFromParams)
	players.RegisteredSearchers = append(players.RegisteredSearchers,
		alphabeta.NewFromParams, mcts.NewFromParams)

	goAI, err := players.New(*flagAI)
	if err != nil {
		fmt.Fprintf(os.Stderr, "building hiveGo AI from %q: %v\n", *flagAI, err)
		os.Exit(1)
	}

	r, err := startRust(*flagRust)
	if err != nil {
		fmt.Fprintf(os.Stderr, "starting Rust engine: %v\n", err)
		os.Exit(1)
	}
	defer r.close()

	rng := rand.New(rand.NewSource(*flagSeed))

	if *flagFuzz > 0 {
		if err := fuzz(r, rng, *flagFuzz); err != nil {
			fmt.Fprintf(os.Stderr, "\n%v\n", err)
			os.Exit(1)
		}
		fmt.Printf("fuzz: %d random playouts, move generators agreed at every ply\n", *flagFuzz)
		return
	}

	var t tally
	start := time.Now()

	fmt.Printf("hive-engine (think=%v) vs hiveGo (%s)\n", *flagThink, *flagAI)
	fmt.Printf("%d games, %d random opening plies, max %d plies, seed %d, validate=%v\n\n",
		*flagGames, *flagOpening, *flagMaxMoves, *flagSeed, *flagValidate)

	for g := 0; g < *flagGames; g++ {
		rustIsFirst := g%2 == 0
		res, err := playGame(r, goAI, rustIsFirst, rng, &t)
		if err != nil {
			fmt.Fprintf(os.Stderr, "\ngame %d failed: %v\n", g+1, err)
			os.Exit(1)
		}
		switch res {
		case 1:
			t.rustWins++
			if rustIsFirst {
				t.rustWinsAsFirst++
			} else {
				t.rustWinsAsSecond++
			}
		case -1:
			t.goWins++
		default:
			t.draws++
		}
		seat := "2nd"
		if rustIsFirst {
			seat = "1st"
		}
		fmt.Printf("game %3d (rust %s): %-6s   running: rust %d - %d hiveGo, %d draws\n",
			g+1, seat, map[int]string{1: "rust", -1: "hiveGo", 0: "draw"}[res],
			t.rustWins, t.goWins, t.draws)
	}

	decisive := t.rustWins + t.goWins
	fmt.Printf("\n=== %d games in %v ===\n", *flagGames, time.Since(start).Round(time.Second))
	fmt.Printf("hive-engine: %d wins (%d as 1st, %d as 2nd)\n", t.rustWins, t.rustWinsAsFirst, t.rustWinsAsSecond)
	fmt.Printf("hiveGo:      %d wins\n", t.goWins)
	fmt.Printf("draws:       %d\n", t.draws)
	for reason, n := range t.drawReasons {
		fmt.Printf("  %-50s %d\n", reason, n)
	}
	if decisive > 0 {
		fmt.Printf("score (draws=½): %.1f%%\n", 100*(float64(t.rustWins)+0.5*float64(t.draws))/float64(*flagGames))
	}
	if t.rustMoves > 0 {
		fmt.Printf("hive-engine avg depth %.1f, avg %d nodes/move\n",
			float64(t.rustDepthSum)/float64(t.rustMoves), t.rustNodes/t.rustMoves)
	}
}
