# AutoLISP

CADCraft runs AutoLISP routines (`.lsp` files). What is here and what is not yet:
[gaps.md](gaps.md) (E1).

## Loading and running

- **APPLOAD** (Tools > Load Application…, alias `AP`) asks for a `.lsp` file and runs it. Dropping
  a `.lsp` file on the window does the same.
- **`(load "name")`** loads a file by path, with `.lsp` added when the name has none. A file
  APPLOAD loaded is found again by its name, also on the web where there is no file system.
- **`(defun c:NAME …)`** defines a command: type `NAME` at the command line. Built-in commands
  keep their names.
- **At the command line**, `(expression)` evaluates and shows the value, and `!variable` shows a
  variable. Typed at a command's prompt (not a free-text one), an expression's value answers it:
  `CIRCLE 0,0 (* 2 r)`.
- **Programmatically** (scripts, the control channel, MCP, the CLI): `lisp {code, inputs?}` and
  `appload {path} | {name, text | data}` run to the end and return `{value, output, error?}`.
  `inputs` answer `get…` calls in order (numbers, strings, `[x, y]` points); a call with none left
  gets nil.

### At startup

CADCraft loads LISP from its **support folder**, `support` in the settings folder (on Windows
`%APPDATA%\CADCraft\support`, on macOS `~/Library/Application Support/CADCraft/support`, on
Linux `~/.config/cadcraft/support`; created at first start):

- `acad.lsp` once per session, into the first drawing; with `ACADLSPASDOC` set to 1, into every
  drawing;
- `acaddoc.lsp` into every drawing opened or created;
- then `S::STARTUP`, when they define it.

`(autoload "file" '("CMD1" "CMD2"))` in a startup file makes `CMD1` and `CMD2` commands that load
`file` (from the support folder too) the first time they are typed. Startup files run to the end
without asking for input. A drawing's own folder is never searched: an `acaddoc.lsp` that came
with a downloaded drawing does not run.

Each drawing has its own variables and functions. A routine is one undo step, whatever commands
it runs. A routine that waits for input (`getpoint`, `pause`) lets the window work meanwhile;
Esc stops it with "Function cancelled", which `*error*` receives. A long loop runs a slice per
frame, so the window stays live and Esc stops it too.

## The language

Integers are 32-bit and wrap as AutoLISP's do; any real makes arithmetic real. Variables are
dynamically scoped: a function's arguments and its locals (after `/`) are bound while it runs.
Symbols are case-insensitive. Reals print with six significant digits.

Nothing a routine does can crash the program: deep recursion is the error "internal stack limit
reached", malformed source is a reader error, and every built-in checks its arguments.

## Built-in functions

- **Forms:** `quote` `'` `function` `setq` `set` `if` `cond` `while` `repeat` `foreach` `progn`
  `and` `or` `defun` `defun-q` `lambda` `eval` `apply` `mapcar` `load` `autoload` `exit` `quit`
- **Numbers:** `+ - * / rem 1+ 1- abs min max sqrt exp log sin cos atan expt fix float gcd
  logand logior lsh ~ = /= < > <= >= zerop minusp numberp`
- **Lists:** `car cdr` and the `c…r` combinations up to four levels, `cons list append reverse
  length nth last member assoc subst acad_strlsort`, `vl-position vl-remove vl-sort
  vl-remove-if vl-remove-if-not vl-member-if vl-every vl-some vl-list-length vl-consp`
- **Predicates:** `atom listp null not type eq equal boundp vl-symbolp`
- **Strings:** `strcat strlen substr strcase itoa atoi atof chr ascii read wcmatch`,
  `vl-string-search vl-string-subst vl-string-trim vl-string-left-trim vl-string-right-trim
  vl-string-translate vl-string->list vl-list->string vl-prin1-to-string vl-princ-to-string
  vl-symbol-name vl-symbol-value vl-filename-base vl-filename-extension vl-filename-directory`
- **Strings, more:** `vl-string-position vl-string-elt vl-string-mismatch`
- **Numbers, more:** `boole`
- **Geometry:** `polar distance angle inters trans` (`trans` returns the point: no UCS yet)
- **Curves:** `vlax-curve-getStartParam vlax-curve-getEndParam vlax-curve-getStartPoint
  vlax-curve-getEndPoint vlax-curve-getDistAtParam vlax-curve-getParamAtDist
  vlax-curve-getPointAtParam vlax-curve-getPointAtDist vlax-curve-getParamAtPoint
  vlax-curve-getDistAtPoint vlax-curve-getClosestPointTo vlax-curve-getClosestPointToProjection
  vlax-curve-getFirstDeriv vlax-curve-getSecondDeriv vlax-curve-getArea vlax-curve-isClosed
  vlax-curve-isPeriodic vlax-curve-isPlanar`, on lines, arcs, circles, polylines (2D and 3D),
  ellipses and splines, given by entity name. Parameters are AutoLISP's: a line's is the
  distance along it, an arc's or a circle's the angle, a polyline's the vertex index (plus the
  fraction of the segment after it), an ellipse's or a spline's its own. Ellipses and splines
  are exact too: lengths and areas are integrated along the curve itself, the same lengths
  LENGTHEN measures.
- **Output and files:** `princ prin1 print prompt terpri alert open close read-line write-line
  read-char write-char findfile`
- **Files and folders:** `vl-file-copy` (never over an existing file; appends with its third
  argument) `vl-file-delete vl-file-rename vl-file-size vl-file-systime` (UTC)
  `vl-file-directory-p vl-directory-files` (sorted) `vl-mkdir`. On the web, where there is no
  file system, they answer nil.
- **Environment:** `getenv setenv`. What `setenv` sets lasts while CADCraft runs; `getenv` falls
  back to the operating system's variables.
- **Object snaps:** `osnap` (`"_end,_mid"`, the same names as at a prompt), within the aperture.
- **Names:** `getcname` (`"line"` gives `"_LINE"`, `"_line"` gives `"LINE"`), `snvalid`,
  `atoms-family`
- **Errors:** `*error*`, `vl-catch-all-apply vl-catch-all-error-p vl-catch-all-error-message`
- **Drawing:** `command` (also `command-s`, `vl-cmdf`) with `pause`, a command left open for the
  next call, and `(command)` to cancel; `getvar setvar`; `entget entmake entmod entdel entlast
  entnext entupd handent`; `ssget` (modes X, L, P, I, W, C, F, WP, CP, a point, or the user
  selecting; filters with wildcards and `-4` operators) `ssadd ssdel sslength ssname ssmemb
  sssetfirst ssgetfirst`; `tblsearch tblnext` (LAYER, LTYPE, STYLE, BLOCK, DIMSTYLE, APPID)
- **Input:** `initget getpoint getcorner getdist getangle getorient getreal getint getstring
  getkword entsel nentsel getfiled`. `getfiled` asks for the name at the command line, as with
  FILEDIA 0 (no dialog opens from LISP): a bare name gets the extension, Enter takes the default
  (nil without one), and unless flag 1 asks for a new file the file must exist.
- **Units:** `rtos angtos distof angtof`
- **No-ops:** `vl-load-com gc redraw textscr graphscr textpage acad-push-dbmod acad-pop-dbmod`;
  `ver`

Entity lists use the DXF group codes of CADCraft's DXF files: what `entget` returns is what a
DXF file holds for the object, plus `-1` (its name) and `410` (its layout).

## Testing against the corpus

The AutoLISP conformance corpus (#648) lives in its own repository. Point `CADCRAFT_LISP_CORPUS`
at it and run `cargo test -p cadcraft-engine --test lisp_corpus`; without the variable the test
says it was skipped.
