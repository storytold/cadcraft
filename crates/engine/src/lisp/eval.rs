//! The evaluator: an explicit-stack machine. Nothing recurses on the Rust stack, so a routine
//! can stop in the middle (a `getpoint` waiting for a click, a `pause` inside `command`) and
//! carry on when the input arrives, on one thread (the web build has no others). Deep recursion
//! in LISP is an error ("internal stack limit reached"), never a crash.
//!
//! Variables are dynamically scoped, as in AutoLISP: a function's arguments and locals are bound
//! on entry and restored when it returns, or when an error unwinds through it.

use super::value::{BuiltinKind, Func, Sym, Value, to_text};
use super::{Env, LispError};
use crate::Session;

/// Frames on the machine's stack: what to do with the value of the expression just evaluated.
pub enum Frame {
    /// Evaluating a call's arguments: `f` gets `done` and then the values of `rest`.
    Args {
        f: Value,
        rest: Value,
        done: Vec<Value>,
    },
    /// The head of a form was itself a form (`((lambda (x) …) 1)`): call its value with `args`.
    Head {
        args: Value,
    },
    Progn {
        rest: Value,
    },
    If {
        then: Value,
        els: Value,
    },
    /// A `cond` clause's test was evaluated: its body runs when true, else the next clauses.
    Cond {
        body: Value,
        rest: Value,
    },
    And {
        rest: Value,
    },
    Or {
        rest: Value,
    },
    Setq {
        sym: Sym,
        rest: Value,
    },
    WhileTest {
        test: Value,
        body: Value,
        last: Value,
    },
    WhileBody {
        test: Value,
        body: Value,
    },
    /// `repeat`'s count was evaluated.
    RepeatCount {
        body: Value,
    },
    Repeat {
        left: i64,
        body: Value,
    },
    /// `foreach`'s list was evaluated.
    ForeachList {
        var: Sym,
        body: Value,
    },
    Foreach {
        var: Sym,
        items: Value,
        body: Value,
        saved: Option<Value>,
    },
    /// A function call ends: restore the bindings its arguments and locals hid.
    Unbind {
        saved: Vec<(Sym, Option<Value>)>,
    },
    /// `mapcar`: `f` is called with the heads of `lists` in turn.
    Mapcar {
        f: Value,
        lists: Vec<Value>,
        acc: Vec<Value>,
    },
    /// Top-level forms of a loaded file or a command line, evaluated in turn.
    Forms {
        rest: Vec<Value>,
    },
    /// `vl-catch-all-apply`: an error below this frame becomes its value.
    Catch,
    /// After a wait, call `f` with `args` (the rest of a `command` after a `pause`).
    Resume {
        f: Value,
        args: Vec<Value>,
    },
}

/// Special forms: their arguments are not evaluated first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Special {
    Quote,
    Function,
    Setq,
    If,
    Cond,
    While,
    Repeat,
    Foreach,
    Progn,
    And,
    Or,
    Defun,
    Lambda,
}

/// What a built-in asks of the machine.
pub enum Flow {
    Value(Value),
    /// Call a function with these arguments (`apply`).
    Call(Value, Vec<Value>),
    /// Evaluate an expression (`eval`).
    Eval(Value),
    /// Evaluate forms in order; the value is the last one's (`load`).
    Forms(Vec<Value>),
    /// `mapcar`.
    Map(Value, Vec<Value>),
    /// `vl-catch-all-apply`: call the function, turning an error into a value.
    Catch(Value, Vec<Value>),
    /// Stop until the session supplies a value (see [`Need`]).
    Wait(Need),
    /// Stop, then call the function with these arguments.
    WaitThen(Need, Value, Vec<Value>),
}

/// Why the machine stopped before finishing.
#[derive(Clone, Debug)]
pub enum Need {
    /// A `get…` function waits for the user.
    Get(super::input::GetRequest),
    /// `pause` inside `command`: the running command takes one input from the user.
    Pause,
    /// The step budget ran out: let the interface draw a frame, then carry on.
    Yield,
}

/// How a run ended.
pub enum Outcome {
    Done(Value),
    Wait(Need),
    Error(LispError),
}

/// What builtins get: the session and the LISP environment.
pub struct Ctx<'a> {
    pub s: &'a mut Session,
    pub env: &'a mut Env,
    /// Nesting depth of [`Ctx::call_sync`].
    pub depth: usize,
}

/// Deepest stack the machine allows, in frames: well above real routines, well below memory
/// trouble.
pub const MAX_STACK: usize = 100_000;
/// Nesting depth of synchronous callbacks (`vl-sort` comparing with a LISP function, …).
const MAX_SYNC_DEPTH: usize = 32;
/// Steps per slice while driving a synchronous callback ([`Ctx::call_sync`]).
const SYNC_SLICE: u64 = 400_000;
enum State {
    Eval(Value),
    Ret(Value),
    Call(Value, Vec<Value>),
}

pub struct Machine {
    stack: Vec<Frame>,
    state: State,
    need: Option<Need>,
}

impl Machine {
    /// A machine evaluating `forms` in turn.
    pub fn forms(forms: Vec<Value>) -> Machine {
        Machine { stack: vec![Frame::Forms { rest: forms.into_iter().rev().collect() }], state: State::Ret(Value::Nil), need: None }
    }

    /// A machine calling `f` with `args`.
    pub fn calling(f: Value, args: Vec<Value>) -> Machine {
        Machine { stack: Vec::new(), state: State::Call(f, args), need: None }
    }

    /// Give the value a [`Need`] waited for, and carry on.
    pub fn resume(&mut self, v: Value) {
        self.state = State::Ret(v);
    }

    /// Run until done, waiting, or `budget` steps.
    pub fn run(&mut self, ctx: &mut Ctx<'_>, budget: u64) -> Outcome {
        let mut n = 0u64;
        loop {
            n += 1;
            if n > budget {
                return Outcome::Wait(Need::Yield);
            }
            let r = match std::mem::replace(&mut self.state, State::Ret(Value::Nil)) {
                State::Eval(expr) => self.eval(ctx, expr),
                State::Call(f, args) => self.call(ctx, f, args),
                State::Ret(v) => match self.stack.pop() {
                    None => return Outcome::Done(v),
                    Some(frame) => self.ret(ctx, frame, v),
                },
            };
            // A runaway routine stops before it takes all memory.
            let r = r.and_then(|()| super::value::check_cells());
            if let Err(e) = r {
                self.need = None;
                if let Some(o) = self.unwind(ctx, e) {
                    return o;
                }
            } else if self.stack.len() > MAX_STACK {
                if let Some(o) = self.unwind(ctx, LispError::new("internal stack limit reached")) {
                    return o;
                }
            } else if let Some(need) = self.need.take() {
                return Outcome::Wait(need);
            }
        }
    }

    /// An error: pop frames back to a `vl-catch-all-apply` (its value becomes the error), or out
    /// of the run. Bindings are restored on the way.
    fn unwind(&mut self, ctx: &mut Ctx<'_>, e: LispError) -> Option<Outcome> {
        while let Some(frame) = self.stack.pop() {
            match frame {
                Frame::Unbind { saved } => restore(ctx.env, saved),
                Frame::Foreach { var, saved, .. } => restore(ctx.env, vec![(var, saved)]),
                Frame::Catch if !e.quit => {
                    self.state = State::Ret(super::builtins::catch_all_error(&e.msg));
                    return None;
                }
                _ => {}
            }
        }
        Some(Outcome::Error(e))
    }

    fn eval(&mut self, ctx: &mut Ctx<'_>, expr: Value) -> Result<(), LispError> {
        match expr {
            Value::Sym(s) => self.state = State::Ret(lookup(ctx.env, &s)),
            Value::Cons(c) => {
                let (head, args) = (c.car.clone(), c.cdr.clone());
                let f = match &head {
                    Value::Sym(s) => {
                        let f = lookup(ctx.env, s);
                        if f.is_nil() {
                            return Err(LispError::new(format!("no function definition: {}", s.name())));
                        }
                        f
                    }
                    Value::Cons(_) => {
                        self.stack.push(Frame::Head { args });
                        self.state = State::Eval(head);
                        return Ok(());
                    }
                    other => other.clone(),
                };
                return self.apply_form(ctx, f, args);
            }
            atom => self.state = State::Ret(atom),
        }
        Ok(())
    }

    /// A form whose head evaluated to `f`: a special form, or a call with `args` to evaluate.
    fn apply_form(&mut self, ctx: &mut Ctx<'_>, f: Value, args: Value) -> Result<(), LispError> {
        if let Value::Subr(b) = &f
            && let BuiltinKind::Special(sp) = b.kind
        {
            return self.special(ctx, sp, args);
        }
        if !matches!(f, Value::Subr(_) | Value::Func(_)) {
            return Err(LispError::new(format!("bad function: {}", to_text(&f, true))));
        }
        match args {
            Value::Cons(c) => {
                self.stack.push(Frame::Args { f, rest: c.cdr.clone(), done: Vec::new() });
                self.state = State::Eval(c.car.clone());
            }
            _ => self.state = State::Call(f, Vec::new()),
        }
        Ok(())
    }

    fn ret(&mut self, ctx: &mut Ctx<'_>, frame: Frame, v: Value) -> Result<(), LispError> {
        match frame {
            Frame::Args { f, rest, mut done } => {
                done.push(v);
                match rest {
                    Value::Cons(c) => {
                        self.stack.push(Frame::Args { f, rest: c.cdr.clone(), done });
                        self.state = State::Eval(c.car.clone());
                    }
                    _ => self.state = State::Call(f, done),
                }
            }
            Frame::Head { args } => return self.apply_form(ctx, v, args),
            Frame::Progn { rest } => self.progn(rest, v),
            Frame::If { then, els } => self.state = State::Eval(if v.is_nil() { els } else { then }),
            Frame::Cond { body, rest } => {
                if v.is_nil() {
                    self.cond(rest);
                } else if body.is_nil() {
                    self.state = State::Ret(v);
                } else {
                    self.progn(body, Value::Nil);
                }
            }
            Frame::And { rest } => {
                if v.is_nil() {
                    self.state = State::Ret(Value::Nil);
                } else {
                    self.and_or(rest, true);
                }
            }
            Frame::Or { rest } => {
                if v.is_nil() {
                    self.and_or(rest, false);
                } else {
                    self.state = State::Ret(Value::t());
                }
            }
            Frame::Setq { sym, rest } => {
                ctx.env.set(&sym, v.clone());
                self.setq(rest, v)?;
            }
            Frame::WhileTest { test, body, last } => {
                if v.is_nil() {
                    self.state = State::Ret(last);
                } else {
                    self.stack.push(Frame::WhileBody { test, body: body.clone() });
                    self.progn(body, Value::Nil);
                }
            }
            Frame::WhileBody { test, body } => {
                self.stack.push(Frame::WhileTest { test: test.clone(), body, last: v });
                self.state = State::Eval(test);
            }
            Frame::RepeatCount { body } => {
                let Value::Int(n) = v else { return Err(LispError::bad_arg("fixnump", &v)) };
                self.repeat(i64::from(n), body, Value::Nil);
            }
            Frame::Repeat { left, body } => self.repeat(left, body, v),
            Frame::ForeachList { var, body } => {
                if !v.is_list() {
                    return Err(LispError::bad_arg("listp", &v));
                }
                let saved = ctx.env.globals.get(&var).cloned();
                self.foreach(ctx, var, v, body, saved, Value::Nil);
            }
            Frame::Foreach { var, items, body, saved } => self.foreach(ctx, var, items, body, saved, v),
            Frame::Unbind { saved } => {
                restore(ctx.env, saved);
                self.state = State::Ret(v);
            }
            Frame::Mapcar { f, lists, mut acc } => {
                acc.push(v);
                let lists: Vec<Value> = lists.iter().map(Value::cdr).collect();
                if lists.iter().any(|l| !matches!(l, Value::Cons(_))) {
                    self.state = State::Ret(Value::list(acc));
                } else {
                    let args = lists.iter().map(Value::car).collect();
                    self.stack.push(Frame::Mapcar { f: f.clone(), lists, acc });
                    self.state = State::Call(f, args);
                }
            }
            Frame::Forms { mut rest } => match rest.pop() {
                Some(next) => {
                    self.stack.push(Frame::Forms { rest });
                    self.state = State::Eval(next);
                }
                None => self.state = State::Ret(v),
            },
            Frame::Catch => self.state = State::Ret(v),
            Frame::Resume { f, args } => self.state = State::Call(f, args),
        }
        Ok(())
    }

    /// Evaluate the forms of `body` in turn; `last` is the value when there are none.
    fn progn(&mut self, body: Value, last: Value) {
        match body {
            Value::Cons(c) => {
                if !c.cdr.is_nil() {
                    self.stack.push(Frame::Progn { rest: c.cdr.clone() });
                }
                self.state = State::Eval(c.car.clone());
            }
            _ => self.state = State::Ret(last),
        }
    }

    fn cond(&mut self, clauses: Value) {
        match clauses {
            Value::Cons(c) => {
                let clause = c.car.clone();
                self.stack.push(Frame::Cond { body: clause.cdr(), rest: c.cdr.clone() });
                self.state = State::Eval(clause.car());
            }
            _ => self.state = State::Ret(Value::Nil),
        }
    }

    /// The next operand of `and` (`and_`) or `or`.
    fn and_or(&mut self, rest: Value, and_: bool) {
        match rest {
            Value::Cons(c) => {
                self.stack.push(if and_ { Frame::And { rest: c.cdr.clone() } } else { Frame::Or { rest: c.cdr.clone() } });
                self.state = State::Eval(c.car.clone());
            }
            _ => self.state = State::Ret(Value::bool(and_)),
        }
    }

    fn setq(&mut self, pairs: Value, last: Value) -> Result<(), LispError> {
        match pairs {
            Value::Cons(c) => {
                let Value::Sym(sym) = c.car.clone() else {
                    return Err(LispError::bad_arg("symbolp", &c.car));
                };
                let Value::Cons(rest) = c.cdr.clone() else { return Err(LispError::new("too few arguments")) };
                self.stack.push(Frame::Setq { sym, rest: rest.cdr.clone() });
                self.state = State::Eval(rest.car.clone());
            }
            _ => self.state = State::Ret(last),
        }
        Ok(())
    }

    fn repeat(&mut self, left: i64, body: Value, last: Value) {
        if left <= 0 || body.is_nil() {
            self.state = State::Ret(last);
        } else {
            self.stack.push(Frame::Repeat { left: left - 1, body: body.clone() });
            self.progn(body, Value::Nil);
        }
    }

    fn foreach(&mut self, ctx: &mut Ctx<'_>, var: Sym, items: Value, body: Value, saved: Option<Value>, last: Value) {
        match items {
            Value::Cons(c) => {
                ctx.env.set(&var, c.car.clone());
                self.stack.push(Frame::Foreach { var, items: c.cdr.clone(), body: body.clone(), saved });
                self.progn(body, Value::Nil);
            }
            _ => {
                restore(ctx.env, vec![(var, saved)]);
                self.state = State::Ret(last);
            }
        }
    }

    fn special(&mut self, ctx: &mut Ctx<'_>, sp: Special, args: Value) -> Result<(), LispError> {
        match sp {
            Special::Quote => self.state = State::Ret(args.car()),
            Special::Function => {
                let a = args.car();
                self.state = State::Ret(if a.car().as_sym().is_some_and(|s| s.name() == "LAMBDA") { make_func(None, a.cdr())? } else { a });
            }
            Special::Setq => {
                if args.is_nil() {
                    return Err(LispError::new("too few arguments"));
                }
                self.setq(args, Value::Nil)?;
            }
            Special::If => {
                let v = args.to_vec();
                if v.len() < 2 {
                    return Err(LispError::new("too few arguments"));
                }
                if v.len() > 3 {
                    return Err(LispError::new("too many arguments"));
                }
                let mut it = v.into_iter();
                let test = it.next().unwrap_or_default();
                let then = it.next().unwrap_or_default();
                let els = it.next().unwrap_or_default();
                self.stack.push(Frame::If { then, els });
                self.state = State::Eval(test);
            }
            Special::Cond => self.cond(args),
            Special::While => {
                let test = args.car();
                self.stack.push(Frame::WhileTest { test: test.clone(), body: args.cdr(), last: Value::Nil });
                self.state = State::Eval(test);
            }
            Special::Repeat => {
                self.stack.push(Frame::RepeatCount { body: args.cdr() });
                self.state = State::Eval(args.car());
            }
            Special::Foreach => {
                let Value::Sym(var) = args.car() else { return Err(LispError::bad_arg("symbolp", &args.car())) };
                self.stack.push(Frame::ForeachList { var, body: args.cdr().cdr() });
                self.state = State::Eval(args.cdr().car());
            }
            Special::Progn => self.progn(args, Value::Nil),
            Special::And => self.and_or(args, true),
            Special::Or => self.and_or(args, false),
            Special::Defun => {
                let Value::Sym(name) = args.car() else { return Err(LispError::bad_arg("symbolp", &args.car())) };
                let f = make_func(Some(name.clone()), args.cdr())?;
                ctx.env.set(&name, f);
                self.state = State::Ret(Value::Sym(name));
            }
            Special::Lambda => self.state = State::Ret(make_func(None, args)?),
        }
        Ok(())
    }

    /// Call `f` with evaluated `args`.
    fn call(&mut self, ctx: &mut Ctx<'_>, f: Value, args: Vec<Value>) -> Result<(), LispError> {
        match &f {
            Value::Subr(b) => match b.kind {
                BuiltinKind::Pure(func) => {
                    let v = func(&args)?;
                    super::value::charge_result(&v)?;
                    self.state = State::Ret(v);
                }
                BuiltinKind::Ctx(func) => {
                    let flow = func(ctx, args)?;
                    if let Flow::Value(v) = &flow {
                        super::value::charge_result(v)?;
                    }
                    self.flow(flow);
                }
                BuiltinKind::Special(_) => return Err(LispError::new(format!("bad function: {}", b.name))),
            },
            Value::Func(func) => {
                if args.len() < func.params.len() {
                    return Err(LispError::new("too few arguments"));
                }
                if args.len() > func.params.len() {
                    return Err(LispError::new("too many arguments"));
                }
                let mut saved = Vec::with_capacity(func.params.len() + func.locals.len());
                for (p, a) in func.params.iter().zip(args) {
                    saved.push((p.clone(), ctx.env.globals.get(p).cloned()));
                    ctx.env.set(p, a);
                }
                for l in &func.locals {
                    saved.push((l.clone(), ctx.env.globals.get(l).cloned()));
                    ctx.env.set(l, Value::Nil);
                }
                self.stack.push(Frame::Unbind { saved });
                self.progn(func.body.clone(), Value::Nil);
            }
            other => return Err(LispError::new(format!("bad function: {}", to_text(other, true)))),
        }
        Ok(())
    }

    fn flow(&mut self, flow: Flow) {
        match flow {
            Flow::Value(v) => self.state = State::Ret(v),
            Flow::Call(f, args) => self.state = State::Call(f, args),
            Flow::Eval(e) => self.state = State::Eval(e),
            Flow::Forms(forms) => {
                self.stack.push(Frame::Forms { rest: forms.into_iter().rev().collect() });
                self.state = State::Ret(Value::Nil);
            }
            Flow::Map(f, lists) => {
                if lists.is_empty() || lists.iter().any(|l| !matches!(l, Value::Cons(_))) {
                    self.state = State::Ret(Value::Nil);
                } else {
                    let args = lists.iter().map(Value::car).collect();
                    self.stack.push(Frame::Mapcar { f: f.clone(), lists, acc: Vec::new() });
                    self.state = State::Call(f, args);
                }
            }
            Flow::Catch(f, args) => {
                self.stack.push(Frame::Catch);
                self.state = State::Call(f, args);
            }
            Flow::WaitThen(need, f, args) => {
                self.stack.push(Frame::Resume { f, args });
                self.state = State::Ret(Value::Nil);
                self.need = Some(need);
            }
            Flow::Wait(need) => {
                // The value the session supplies replaces this call's.
                self.state = State::Ret(Value::Nil);
                self.need = Some(need);
            }
        }
    }
}

/// A variable's value (nil when unbound).
pub fn lookup(env: &Env, s: &Sym) -> Value {
    env.globals.get(s).cloned().unwrap_or(Value::Nil)
}

fn restore(env: &mut Env, saved: Vec<(Sym, Option<Value>)>) {
    for (s, v) in saved.into_iter().rev() {
        match v {
            Some(v) => {
                env.globals.insert(s, v);
            }
            None => {
                env.globals.remove(&s);
            }
        }
    }
}

/// A function from `((params / locals) body…)`.
fn make_func(name: Option<Sym>, def: Value) -> Result<Value, LispError> {
    let arglist = def.car();
    if !arglist.is_list() {
        return Err(LispError::bad_arg("listp", &arglist));
    }
    let (mut params, mut locals) = (Vec::new(), Vec::new());
    let mut after_slash = false;
    for a in arglist.iter() {
        let Value::Sym(s) = a else { return Err(LispError::bad_arg("symbolp", &a)) };
        if s.name() == "/" {
            after_slash = true;
        } else if after_slash {
            locals.push(s);
        } else {
            params.push(s);
        }
    }
    Ok(Value::Func(std::sync::Arc::new(Func { name, params, locals, body: def.cdr() })))
}

impl Ctx<'_> {
    /// Call `f` to completion here (a comparison for `vl-sort`, a test for `vl-remove-if`…).
    /// It may not wait for input.
    pub fn call_sync(&mut self, f: &Value, args: Vec<Value>) -> Result<Value, LispError> {
        if self.depth >= MAX_SYNC_DEPTH {
            return Err(LispError::new("internal stack limit reached"));
        }
        if let Value::Subr(b) = f
            && let BuiltinKind::Pure(func) = b.kind
        {
            let v = func(&args)?;
            super::value::charge_result(&v)?;
            return Ok(v);
        }
        let cap = self.s.lisp.limits.callback_steps;
        let mut m = Machine::calling(f.clone(), args);
        let mut sub = Ctx { s: &mut *self.s, env: &mut *self.env, depth: self.depth + 1 };
        // Drive the callback in bounded slices instead of granting it `u64::MAX` fuel. A `Yield`
        // (the step budget ran out) is retried until a hard cap, so a constant-memory infinite
        // loop inside a comparator or predicate terminates with an error rather than hanging the
        // thread and bypassing the blocking execution cap (CWE-400).
        let mut spent: u64 = 0;
        loop {
            let remaining = cap.saturating_sub(spent);
            if remaining == 0 {
                return Err(LispError::new(format!("callback stopped after {cap} steps")));
            }
            let slice = remaining.min(SYNC_SLICE);
            match m.run(&mut sub, slice) {
                Outcome::Done(v) => return Ok(v),
                Outcome::Error(e) => return Err(e),
                Outcome::Wait(Need::Yield) => {
                    spent = spent.saturating_add(slice);
                    if spent >= cap {
                        return Err(LispError::new(format!("callback stopped after {cap} steps")));
                    }
                }
                Outcome::Wait(_) => return Err(LispError::new("this function can't ask for input here")),
            }
        }
    }
}
