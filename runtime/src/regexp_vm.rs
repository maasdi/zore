use super::regexp_parse::{self, Assertion, ClassItem, MAX_CAPTURES, Matcher, Node};

pub(super) const MAX_STATES: usize = 4096;
pub(super) const MAX_WORK: usize = 16_777_216;
pub(super) const MAX_INPUT: usize = 1_048_576;
pub(super) const MAX_MATCHES: usize = 1024;
pub(super) const MAX_OUTPUT: usize = 1_048_576;
const SLOTS: usize = (MAX_CAPTURES + 1) * 2;
const ABSENT: usize = usize::MAX;
type Captures = [usize; SLOTS];

#[derive(Debug)]
enum Instruction {
    Consume(Matcher, usize),
    Assert(Assertion, usize),
    Save(usize, usize),
    Split(usize, usize),
    Accept,
}

#[derive(Debug)]
pub(super) struct Program {
    instructions: Vec<Instruction>,
    entry: usize,
    pub captures: usize,
}

pub(super) struct Budget(usize);

impl Budget {
    pub fn new() -> Self {
        Self(MAX_WORK)
    }

    fn charge(&mut self) -> Result<(), String> {
        self.0 = self.0.checked_sub(1).ok_or("work limit exceeded")?;
        Ok(())
    }
}

struct Builder {
    instructions: Vec<Instruction>,
    budget: Budget,
}

impl Builder {
    fn emit(&mut self, instruction: Instruction) -> Result<usize, String> {
        if self.instructions.len() == MAX_STATES {
            return Err("compiled state limit exceeded".into());
        }
        let index = self.instructions.len();
        self.instructions.push(instruction);
        Ok(index)
    }

    fn split(&mut self, body: usize, next: usize, greedy: bool) -> Result<usize, String> {
        self.emit(if greedy {
            Instruction::Split(body, next)
        } else {
            Instruction::Split(next, body)
        })
    }

    fn build(&mut self, node: &Node, next: usize) -> Result<usize, String> {
        self.budget.charge()?;
        match node {
            Node::Empty => Ok(next),
            Node::Consume(matcher) => self.emit(Instruction::Consume(matcher.clone(), next)),
            Node::Assert(assertion) => self.emit(Instruction::Assert(*assertion, next)),
            Node::Sequence(nodes) => {
                let mut at = next;
                for node in nodes.iter().rev() {
                    at = self.build(node, at)?;
                }
                Ok(at)
            }
            Node::Alternate(arms) => {
                let mut arms = arms.iter().rev();
                let mut at = self.build(arms.next().unwrap(), next)?;
                for arm in arms {
                    let first = self.build(arm, next)?;
                    at = self.split(first, at, true)?;
                }
                Ok(at)
            }
            Node::Group(None, body) => self.build(body, next),
            Node::Group(Some(index), body) => {
                let end = self.emit(Instruction::Save(index * 2 + 1, next))?;
                let body = self.build(body, end)?;
                self.emit(Instruction::Save(index * 2, body))
            }
            Node::Repeat {
                body,
                min,
                max,
                greedy,
            } => {
                let mut at = next;
                match max {
                    Some(max) => {
                        for _ in *min..*max {
                            let body = self.build(body, at)?;
                            at = self.split(body, next, *greedy)?;
                        }
                    }
                    None => {
                        let loop_at = self.emit(Instruction::Split(next, next))?;
                        let body = self.build(body, loop_at)?;
                        self.instructions[loop_at] = if *greedy {
                            Instruction::Split(body, next)
                        } else {
                            Instruction::Split(next, body)
                        };
                        at = loop_at;
                    }
                }
                for _ in 0..*min {
                    at = self.build(body, at)?;
                }
                Ok(at)
            }
        }
    }
}

#[derive(Clone)]
struct Thread {
    pc: usize,
    captures: Captures,
}

struct Frontier {
    threads: Vec<Thread>,
    seen: Vec<usize>,
    generation: usize,
}

impl Frontier {
    fn new(states: usize) -> Self {
        Self {
            threads: Vec::new(),
            seen: vec![0; states],
            generation: 1,
        }
    }

    fn clear(&mut self) {
        self.threads.clear();
        self.generation += 1;
    }
}

fn word(ch: Option<char>) -> bool {
    ch.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn assertion_matches(assertion: Assertion, text: &str, at: usize) -> bool {
    match assertion {
        Assertion::Start => at == 0,
        Assertion::End => at == text.len(),
        Assertion::Word(want) => {
            let before = word(text[..at].chars().next_back());
            let after = word(text[at..].chars().next());
            (before != after) == want
        }
    }
}

fn ascii_matches(kind: char, ch: char) -> bool {
    let matches = match kind.to_ascii_lowercase() {
        'd' => ch.is_ascii_digit(),
        'w' => word(Some(ch)),
        's' => matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{c}' | '\u{b}'),
        _ => unreachable!(),
    };
    matches != kind.is_ascii_uppercase()
}

fn matches(matcher: &Matcher, ch: char, budget: &mut Budget) -> Result<bool, String> {
    budget.charge()?;
    Ok(match matcher {
        Matcher::Literal(literal) => ch == *literal,
        Matcher::Dot => ch != '\n',
        Matcher::Class(class) => {
            let mut found = false;
            for item in &class.items {
                budget.charge()?;
                found = match item {
                    ClassItem::Range(first, last) => (*first..=*last).contains(&ch),
                    ClassItem::Ascii(kind) => ascii_matches(*kind, ch),
                };
                if found {
                    break;
                }
            }
            found != class.negated
        }
    })
}

impl Program {
    pub fn compile(pattern: &str) -> Result<Self, String> {
        let (node, captures) = regexp_parse::parse(pattern)?;
        let mut builder = Builder {
            instructions: Vec::new(),
            budget: Budget::new(),
        };
        let accept = builder.emit(Instruction::Accept)?;
        let end = builder.emit(Instruction::Save(1, accept))?;
        let body = builder.build(&node, end)?;
        let entry = builder.emit(Instruction::Save(0, body))?;
        Ok(Self {
            instructions: builder.instructions,
            entry,
            captures,
        })
    }

    fn add_thread(
        &self,
        thread: Thread,
        at: usize,
        text: &str,
        frontier: &mut Frontier,
        stack: &mut Vec<Thread>,
        budget: &mut Budget,
    ) -> Result<(), String> {
        stack.push(thread);
        while let Some(mut thread) = stack.pop() {
            budget.charge()?;
            if frontier.seen[thread.pc] == frontier.generation {
                continue;
            }
            frontier.seen[thread.pc] = frontier.generation;
            match &self.instructions[thread.pc] {
                Instruction::Consume(..) | Instruction::Accept => frontier.threads.push(thread),
                Instruction::Assert(assertion, next) => {
                    if assertion_matches(*assertion, text, at) {
                        thread.pc = *next;
                        stack.push(thread);
                    }
                }
                Instruction::Save(slot, next) => {
                    thread.captures[*slot] = at;
                    thread.pc = *next;
                    stack.push(thread);
                }
                Instruction::Split(first, second) => {
                    let mut other = thread.clone();
                    other.pc = *second;
                    stack.push(other);
                    thread.pc = *first;
                    stack.push(thread);
                }
            }
        }
        Ok(())
    }

    fn find(
        &self,
        text: &str,
        start: usize,
        budget: &mut Budget,
    ) -> Result<Option<Captures>, String> {
        let mut current = Frontier::new(self.instructions.len());
        let mut next = Frontier::new(self.instructions.len());
        let mut stack = Vec::new();
        let mut candidate = None;
        let mut at = start;
        loop {
            if candidate.is_none() {
                self.add_thread(
                    Thread {
                        pc: self.entry,
                        captures: [ABSENT; SLOTS],
                    },
                    at,
                    text,
                    &mut current,
                    &mut stack,
                    budget,
                )?;
            }
            let ch = text[at..].chars().next();
            next.clear();
            for thread in current.threads.drain(..) {
                match &self.instructions[thread.pc] {
                    Instruction::Accept => {
                        candidate = Some(thread.captures);
                        break;
                    }
                    Instruction::Consume(matcher, target) => {
                        if let Some(ch) = ch
                            && matches(matcher, ch, budget)?
                        {
                            self.add_thread(
                                Thread {
                                    pc: *target,
                                    captures: thread.captures,
                                },
                                at + ch.len_utf8(),
                                text,
                                &mut next,
                                &mut stack,
                                budget,
                            )?;
                        }
                    }
                    _ => unreachable!(),
                }
            }
            if next.threads.is_empty() && candidate.is_some() {
                return Ok(candidate);
            }
            let Some(ch) = ch else {
                return Ok(candidate);
            };
            at += ch.len_utf8();
            std::mem::swap(&mut current, &mut next);
        }
    }

    pub fn find_all(
        &self,
        text: &str,
        limit: usize,
        budget: &mut Budget,
    ) -> Result<Vec<Captures>, String> {
        let mut found = Vec::new();
        let mut at = 0;
        let mut last_nonempty_end = None;
        while at <= text.len() && found.len() < limit {
            let Some(captures) = self.find(text, at, budget)? else {
                break;
            };
            let (start, end) = (captures[0], captures[1]);
            if start == end {
                if last_nonempty_end != Some(end) {
                    found.push(captures);
                }
                match text[end..].chars().next() {
                    Some(ch) => at = end + ch.len_utf8(),
                    None => break,
                }
            } else {
                at = end;
                last_nonempty_end = Some(end);
                found.push(captures);
            }
        }
        Ok(found)
    }

    pub fn offsets(&self, captures: &Captures) -> impl Iterator<Item = i64> {
        captures[..(self.captures + 1) * 2]
            .iter()
            .map(|&at| if at == ABSENT { -1 } else { at as i64 })
    }

    pub fn replace(&self, text: &str, replacement: &str, literal: bool) -> Result<String, String> {
        let parts = if literal {
            vec![Part::Literal(replacement)]
        } else {
            replacement_parts(replacement, self.captures)?
        };
        let found = self.find_all(text, MAX_MATCHES + 1, &mut Budget::new())?;
        if found.len() > MAX_MATCHES {
            return Err("match limit exceeded".into());
        }
        let mut result = String::new();
        let mut at = 0;
        for captures in found {
            append(&mut result, &text[at..captures[0]])?;
            for part in &parts {
                match part {
                    Part::Literal(value) => append(&mut result, value)?,
                    Part::Group(group) => {
                        let (start, end) = (captures[group * 2], captures[group * 2 + 1]);
                        if start != ABSENT && end != ABSENT {
                            append(&mut result, &text[start..end])?;
                        }
                    }
                }
            }
            at = captures[1];
        }
        append(&mut result, &text[at..])?;
        Ok(result)
    }
}

fn append(output: &mut String, part: &str) -> Result<(), String> {
    if part.len() > MAX_OUTPUT - output.len() {
        return Err("output byte limit exceeded".into());
    }
    output.push_str(part);
    Ok(())
}

enum Part<'a> {
    Literal(&'a str),
    Group(usize),
}

fn replacement_parts(template: &str, groups: usize) -> Result<Vec<Part<'_>>, String> {
    let bytes = template.as_bytes();
    let mut parts = Vec::new();
    let mut at = 0;
    let mut literal = 0;
    while at < bytes.len() {
        if bytes[at] != b'$' {
            at += 1;
            continue;
        }
        if at > literal {
            parts.push(Part::Literal(&template[literal..at]));
        }
        at += 1;
        if bytes.get(at) == Some(&b'$') {
            parts.push(Part::Literal("$"));
            at += 1;
        } else {
            let braced = bytes.get(at) == Some(&b'{');
            at += usize::from(braced);
            let start = at;
            let mut group = 0;
            while let Some(digit) = bytes.get(at).filter(|digit| digit.is_ascii_digit()) {
                group = group * 10 + usize::from(*digit - b'0');
                if group > groups {
                    return Err("replacement references an unknown capture".into());
                }
                at += 1;
            }
            if start == at || (braced && bytes.get(at) != Some(&b'}')) {
                return Err("invalid replacement reference".into());
            }
            at += usize::from(braced);
            parts.push(Part::Group(group));
        }
        literal = at;
    }
    if literal < bytes.len() {
        parts.push(Part::Literal(&template[literal..]));
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offsets(pattern: &str, text: &str) -> Option<Vec<i64>> {
        let program = Program::compile(pattern).unwrap();
        program
            .find(text, 0, &mut Budget::new())
            .unwrap()
            .map(|captures| program.offsets(&captures).collect())
    }

    #[test]
    fn ordered_alternatives_quantifiers_and_captures_select_the_expected_path() {
        for (pattern, text, expected) in [
            ("a|ab", "xab", vec![1, 2]),
            ("ab|a", "xab", vec![1, 3]),
            ("a+", "baaa", vec![1, 4]),
            ("a+?", "baaa", vec![1, 2]),
            ("a{2,3}", "baaaa", vec![1, 4]),
            ("a{2,3}?", "baaaa", vec![1, 3]),
            ("(a+)(a)", "aaa", vec![0, 3, 0, 2, 2, 3]),
            ("(a+?)(a)", "aaa", vec![0, 2, 0, 1, 1, 2]),
            ("(a)?b", "b", vec![0, 1, -1, -1]),
            ("(a*)b", "b", vec![0, 1, 0, 0]),
            ("(ab)+", "abab", vec![0, 4, 2, 4]),
            ("((a)|(b))+", "ba", vec![0, 2, 1, 2, 1, 2, 0, 1]),
            ("(a?)*", "a", vec![0, 1, 0, 1]),
            ("(?:^)*", "a", vec![0, 0]),
            ("a|", "z", vec![0, 0]),
            ("", "é", vec![0, 0]),
            ("(?:ab){0}", "ab", vec![0, 0]),
            ("(a?){0,3}", "", vec![0, 0, 0, 0]),
            ("(a?)*", "", vec![0, 0, -1, -1]),
            (
                r"(\d[^b]+?){0,2}?(?:é+|[ab])",
                "212éé🦀éaa\n",
                vec![0, 7, 2, 5],
            ),
        ] {
            assert_eq!(
                offsets(pattern, text),
                Some(expected),
                "{pattern:?} on {text:?}"
            );
        }
        assert_eq!(offsets("^a$", "ba"), None);
        assert_eq!(offsets("a$", "a\n"), None);
        assert_eq!(offsets("a{2}", "a"), None);
    }

    #[test]
    fn scalar_matching_preserves_original_utf8_byte_offsets_and_ascii_classes() {
        for (pattern, text, expected) in [
            ("(é)(🦀)", "xé🦀", vec![1, 7, 1, 3, 3, 7]),
            ("[α-ω]+", "xαβz", vec![1, 5]),
            (".", "🦀", vec![0, 4]),
            (r"\xE9\u{1F980}", "é🦀", vec![0, 6]),
            (r"\d+", "é12", vec![2, 4]),
            (r"\D+", "é12", vec![0, 2]),
            (r"\W+", "α_", vec![0, 2]),
            (r"\bword\b", "éwordé", vec![2, 6]),
            (r"\Bé\B", "é", vec![0, 2]),
            (r"[\D0]+", "é012", vec![0, 3]),
            ("[^a]+", "\nαa", vec![0, 3]),
            (r"[\]\-]+", "]-x", vec![0, 2]),
            ("[-a]+", "-aa", vec![0, 3]),
            ("[a-]+", "aa-", vec![0, 3]),
        ] {
            assert_eq!(
                offsets(pattern, text),
                Some(expected),
                "{pattern:?} on {text:?}"
            );
        }
        assert_eq!(offsets("^.$", "e\u{301}"), None);
        assert_eq!(offsets("é", "e\u{301}"), None);
        assert_eq!(offsets(r"\w", "α"), None);
        assert_eq!(offsets(r"\d", "١"), None);
        assert_eq!(offsets(".", "\n"), None);
    }

    #[test]
    fn global_empty_matches_advance_scalars_and_skip_adjacent_empty_results() {
        for (pattern, text, expected) in [
            ("", "é🦀", vec![vec![0, 0], vec![2, 2], vec![6, 6]]),
            ("a*", "aa", vec![vec![0, 2]]),
            ("a*", "aé", vec![vec![0, 1], vec![3, 3]]),
            ("^", "abc", vec![vec![0, 0]]),
            ("$", "abc", vec![vec![3, 3]]),
            ("(?:)*", "ab", vec![vec![0, 0], vec![1, 1], vec![2, 2]]),
        ] {
            let program = Program::compile(pattern).unwrap();
            let found = program.find_all(text, 20, &mut Budget::new()).unwrap();
            let found: Vec<Vec<i64>> = found.iter().map(|m| program.offsets(m).collect()).collect();
            assert_eq!(found, expected, "{pattern:?} on {text:?}");
        }
    }

    #[test]
    fn replacement_is_strict_about_references_and_literal_dollars() {
        let program = Program::compile("(a)?(b)").unwrap();
        assert_eq!(
            program.replace("ab b", "${2}:$1:$$:$0", false).unwrap(),
            "b:a:$:ab b::$:b"
        );
        assert_eq!(program.replace("b", r"\$1", false).unwrap(), "\\");
        assert_eq!(program.replace("b", "$99\\", true).unwrap(), "$99\\");
        for replacement in ["$", "$x", "${}", "${1", "${1x}", "$3", "${17}"] {
            assert!(
                program.replace("no matches", replacement, false).is_err(),
                "{replacement:?}"
            );
        }
        let empty = Program::compile("").unwrap();
        assert_eq!(empty.replace("é", "-", false).unwrap(), "-é-");
        assert_eq!(
            Program::compile("a*")
                .unwrap()
                .replace("aa", "x", false)
                .unwrap(),
            "x"
        );
    }

    #[test]
    fn state_work_output_and_match_bounds_are_enforced() {
        assert!(Program::compile("a{1000}").is_ok());
        assert!(Program::compile(&"a".repeat(MAX_STATES - 3)).is_ok());
        assert!(
            Program::compile(&"a".repeat(MAX_STATES - 2))
                .unwrap_err()
                .contains("state limit")
        );
        assert!(
            Program::compile("(?:a{1000}){5}")
                .unwrap_err()
                .contains("state limit")
        );
        let (empty, _) = regexp_parse::parse("(?:(?:){1000}){1000}").unwrap();
        let mut builder = Builder {
            instructions: vec![Instruction::Accept],
            budget: Budget(4),
        };
        assert_eq!(builder.build(&empty, 0), Err("work limit exceeded".into()));
        let program = Program::compile("a+").unwrap();
        assert_eq!(
            program.find("aaa", 0, &mut Budget(0)),
            Err("work limit exceeded".into())
        );
        let input = "a".repeat(MAX_MATCHES + 1);
        let program = Program::compile("a").unwrap();
        assert_eq!(
            program
                .find_all(&input, 1, &mut Budget::new())
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            program.replace(&input, "", false),
            Err("match limit exceeded".into())
        );
        assert_eq!(
            program.replace("aa", &"x".repeat(MAX_OUTPUT), true),
            Err("output byte limit exceeded".into())
        );
        let mut output = "x".repeat(MAX_OUTPUT);
        assert_eq!(
            append(&mut output, "x"),
            Err("output byte limit exceeded".into())
        );
        assert!(append(&mut output, "").is_ok());
    }

    #[test]
    fn counted_alternation_matches_an_exhaustive_small_alphabet() {
        let program = Program::compile("^(a|b){0,4}$").unwrap();
        for size in 0..7 {
            for mut bits in 0..3_usize.pow(size) {
                let mut input = String::new();
                for _ in 0..size {
                    input.push(['a', 'b', 'é'][bits % 3]);
                    bits /= 3;
                }
                let matched = program.find(&input, 0, &mut Budget::new()).unwrap();
                assert_eq!(
                    matched.is_some(),
                    size <= 4 && !input.contains('é'),
                    "{input:?}"
                );
                if let Some(captures) = matched {
                    assert_eq!(&captures[..2], &[0, input.len()]);
                    if size > 0 {
                        assert_eq!(&captures[2..4], &[input.len() - 1, input.len()]);
                    } else {
                        assert_eq!(&captures[2..4], &[ABSENT, ABSENT]);
                    }
                }
            }
        }
    }
}
