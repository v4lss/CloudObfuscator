use crate::names::NameFactory;
use crate::rng::Rng;

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct GuardSet {
    pub debug_protection: bool,
    pub self_defending: bool,
    pub console_trap: bool,
    pub integrity_check: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardKind {
    DebugProtection,
    SelfDefending,
    ConsoleTrap,
    IntegrityCheck,
}

impl GuardKind {
    pub fn label(&self) -> &'static str {
        match self {
            GuardKind::DebugProtection => "debug-protection",
            GuardKind::SelfDefending => "self-defending",
            GuardKind::ConsoleTrap => "console-trap",
            GuardKind::IntegrityCheck => "integrity-check",
        }
    }
}

const CONSOLE_METHODS: [&str; 6] = ["log", "info", "warn", "error", "debug", "trace"];

pub fn from_char_codes(token: &str) -> String {
    token
        .chars()
        .map(|c| format!("String.fromCharCode({})", c as u32))
        .collect::<Vec<_>>()
        .join("+")
}

pub fn debug_protection(rng: &mut Rng, names: &mut NameFactory, decoder: &str) -> String {
    let probe = names.next(rng);
    let begin = names.next(rng);
    let cursor = names.next(rng);
    let limit = names.next(rng);
    let sink = names.next(rng);
    let guard = names.next(rng);
    let caught = names.next(rng);
    let timer = names.next(rng);
    let body = rng.range(200_000, 900_000);
    let tolerance = rng.range(120, 400);

    let timed = format!(
        "(function(){{var {begin}=Date.now(),{cursor}=0,{sink}=0,{limit}={body};\
         for(;{cursor}<{limit};{cursor}++){{{sink}=({sink}+{cursor})%2147483647;}}\
         return Date.now()-{begin}>{tolerance};}})()",
        begin = begin,
        cursor = cursor,
        sink = sink,
        limit = limit,
        body = body,
        tolerance = tolerance
    );

    let trigger = if rng.chance(50) {
        format!(
            "var {timer}=setInterval(function(){{if({probe}){{clearInterval({timer});}}}},{interval});",
            timer = timer,
            probe = probe,
            interval = rng.range(700, 2400)
        )
    } else {
        format!(
            "var {timer}=setTimeout(function(){{if({probe}){{clearTimeout({timer});}}}},{delay});",
            timer = timer,
            probe = probe,
            delay = rng.range(400, 1500)
        )
    };

    format!(
        "var {probe}={timed};var {guard}=function(){{if({probe}){{\
         try{{{decoder}(0);}}catch({caught}){{}}}}}};{trigger}{guard}();",
        probe = probe,
        timed = timed,
        guard = guard,
        decoder = decoder,
        caught = caught,
        trigger = trigger
    )
}

pub fn self_defending(rng: &mut Rng, names: &mut NameFactory, decoder: &str) -> String {
    let fn_name = names.next(rng);
    let source = names.next(rng);
    let token = names.next(rng);
    let marker = names.next(rng);
    let kind = names.next(rng);
    format!(
        "var {fn_name}=function(){{var {source}={decoder}.toString();\
         var {token}={probe},{marker}=typeof {decoder},{kind}=\"function\";\
         if({marker}!=={kind}){{while(1){{}}}}\
         if({source}.indexOf({token})===-1){{while(1){{}}}}\
         if(!{decoder}.length){{while(1){{}}}}}};{fn_name}();",
        fn_name = fn_name,
        decoder = decoder,
        source = source,
        token = token,
        marker = marker,
        kind = kind,
        probe = from_char_codes("function")
    )
}

pub fn console_trap(rng: &mut Rng, names: &mut NameFactory) -> String {
    let sink = names.next(rng);
    let cursor = names.next(rng);
    let methods = names.next(rng);
    let quiet = names.next(rng);
    let mut chosen: Vec<&str> = CONSOLE_METHODS.to_vec();
    for index in (1..chosen.len()).rev() {
        let swap = rng.below(index + 1);
        chosen.swap(index, swap);
    }
    let count = rng.range(3, chosen.len() + 1);
    chosen.truncate(count);
    let list = chosen
        .iter()
        .map(|m| format!("\"{}\"", m))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "var {sink}=typeof console!=\"undefined\"?console:void 0;\
         if({sink}){{var {quiet}=function(){{}},{methods}=[{list}];\
         for(var {cursor}=0;{cursor}<{methods}.length;{cursor}++){{\
         {sink}[{methods}[{cursor}]]={quiet};}}}}",
        sink = sink,
        methods = methods,
        list = list,
        cursor = cursor,
        quiet = quiet
    )
}

pub fn integrity_check(
    rng: &mut Rng,
    names: &mut NameFactory,
    decoder: &str,
    arity: usize,
) -> String {
    let fn_name = names.next(rng);
    let expected = names.next(rng);
    let name_length = names.next(rng);
    let spin = names.next(rng);
    format!(
        "var {fn_name}=function(){{var {expected}={decoder}.length,\
         {name_length}={decoder}.name.length,{spin}=0;\
         if({expected}!=={arity}||{name_length}<1){{while(1){{}}}}\
         if({spin}++){{return 0;}}return 1;}};{fn_name}();",
        fn_name = fn_name,
        decoder = decoder,
        expected = expected,
        name_length = name_length,
        spin = spin,
        arity = arity
    )
}

pub fn all_kinds(set: GuardSet) -> Vec<GuardKind> {
    let mut out = Vec::new();
    if set.debug_protection {
        out.push(GuardKind::DebugProtection);
    }
    if set.self_defending {
        out.push(GuardKind::SelfDefending);
    }
    if set.console_trap {
        out.push(GuardKind::ConsoleTrap);
    }
    if set.integrity_check {
        out.push(GuardKind::IntegrityCheck);
    }
    out
}
