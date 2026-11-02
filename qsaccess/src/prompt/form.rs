// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The values of a command being prompted, and the command string they
//! make.
//!
//! A [`Form`] keeps one value per node and entry. A parameter that takes a
//! list (`max_values` above 1) has one or more entries; its elements and
//! qualifiers have a value for each of its entries. Values start as the
//! definition's defaults.
//!
//! [`Form::command_string`] gives only parameters whose value differs from
//! their default, in the definition's order, as `KEYWORD(value)`:
//!
//! - a qualified name as `LIB/NAME`, a qualifier left out when it is empty
//!   or its default (`*LIBL`) and none after it is given;
//! - elements in order, separated by blanks, `*N` for one left empty with
//!   no default; a list of element groups puts each group in parentheses;
//! - list values separated by blanks;
//! - a character, path or command value quoted (`'...'`, a quote doubled)
//!   when it is empty or holds anything but upper-case letters, digits and
//!   `$#@_.*/`; a special value (`*X`) or a value already in quotes is
//!   written as it is.
//!
//! Constants (`Show::Never`) are never written: the definition supplies
//! them.

use std::collections::HashMap;

use qap_client::proto::ops::command::{CommandDefinition, ParmKind, ParmNode, Role, Show};

/// A command's definition and the values given so far.
#[derive(Clone, Debug)]
pub struct Form {
    pub def: CommandDefinition,
    /// Entries of each list parameter, by node index (1 for the others).
    entries: HashMap<u16, usize>,
    /// Values by node index and entry.
    values: HashMap<(u16, usize), String>,
}

impl Form {
    /// A form with every value at its default.
    pub fn new(def: CommandDefinition) -> Form {
        Form {
            def,
            entries: HashMap::new(),
            values: HashMap::new(),
        }
    }

    /// The command's name, as it is typed.
    pub fn name(&self) -> &str {
        &self.def.name.name
    }

    /// Node `i`.
    pub fn node(&self, i: u16) -> Option<&ParmNode> {
        self.def.nodes.iter().find(|n| n.index == i)
    }

    /// The parameters, in the definition's order.
    pub fn parameters(&self) -> impl Iterator<Item = &ParmNode> {
        self.def.nodes.iter().filter(|n| n.parent.is_none())
    }

    /// `i`'s elements or qualifiers, in order.
    pub fn children(&self, i: u16) -> impl Iterator<Item = &ParmNode> {
        self.def.nodes.iter().filter(move |n| n.parent == Some(i))
    }

    /// The parameter node `i` belongs to.
    pub fn top(&self, mut i: u16) -> u16 {
        while let Some(p) = self.node(i).and_then(|n| n.parent) {
            i = p;
        }
        i
    }

    /// Entries the parameter `param` has: at least one.
    pub fn entries(&self, param: u16) -> usize {
        self.entries.get(&param).copied().unwrap_or(1).max(1)
    }

    /// Whether `param` may take another entry.
    pub fn can_add(&self, param: u16) -> bool {
        self.node(param)
            .is_some_and(|n| n.max_values > 1 && self.entries(param) < usize::from(n.max_values))
    }

    /// Adds an entry to the list parameter `param`, at its defaults.
    pub fn add_entry(&mut self, param: u16) -> bool {
        if !self.can_add(param) {
            return false;
        }
        let n = self.entries(param) + 1;
        self.entries.insert(param, n);
        true
    }

    /// Node `i`'s value in `entry`: what was given, else its default.
    pub fn value(&self, i: u16, entry: usize) -> String {
        match self.values.get(&(i, entry)) {
            Some(v) => v.clone(),
            None => self.default(i, entry),
        }
    }

    /// Node `i`'s default in `entry`: a list's later entries start empty.
    pub fn default(&self, i: u16, entry: usize) -> String {
        let top = self.top(i);
        if entry > 0 && self.node(top).is_some_and(|n| n.max_values > 1) {
            return String::new();
        }
        self.node(i)
            .and_then(|n| n.default.clone())
            .unwrap_or_default()
    }

    /// Sets node `i`'s value in `entry`.
    pub fn set(&mut self, i: u16, entry: usize, value: &str) {
        self.values.insert((i, entry), value.trim().to_owned());
    }

    /// The command string: the name and every parameter that differs from
    /// its default.
    pub fn command_string(&self) -> String {
        let mut out = self.qualified_name();
        for p in self.parameters() {
            if p.show == Show::Never {
                continue;
            }
            let given = self.render_parameter(p, false);
            if given.is_empty() || given == self.render_parameter(p, true) {
                continue;
            }
            out.push(' ');
            out.push_str(&p.keyword);
            out.push('(');
            out.push_str(&given);
            out.push(')');
        }
        out
    }

    /// The command's name, with its library unless that is `*LIBL` or
    /// `QSYS`, where it is found anyway.
    fn qualified_name(&self) -> String {
        let lib = self.def.name.library.trim();
        if lib.is_empty() || lib.eq_ignore_ascii_case("*LIBL") || lib.eq_ignore_ascii_case("QSYS") {
            self.def.name.name.clone()
        } else {
            format!("{lib}/{}", self.def.name.name)
        }
    }

    /// A parameter's value as written inside its parentheses, from what was
    /// given or (`defaults`) from its defaults alone.
    fn render_parameter(&self, p: &ParmNode, defaults: bool) -> String {
        let n = if defaults { 1 } else { self.entries(p.index) };
        let groups = self
            .children(p.index)
            .next()
            .is_some_and(|c| c.role == Role::Element);
        let wrap = groups && p.max_values > 1;
        let parts: Vec<String> = (0..n)
            .map(|e| self.render(p.index, e, defaults))
            .filter(|s| !s.is_empty())
            .map(|s| if wrap { format!("({s})") } else { s })
            .collect();
        parts.join(" ")
    }

    /// Node `i`'s value in `entry`, written out; empty when not given.
    fn render(&self, i: u16, entry: usize, defaults: bool) -> String {
        let Some(node) = self.node(i) else {
            return String::new();
        };
        let kids: Vec<&ParmNode> = self.children(i).collect();
        let v = |j: u16| {
            if defaults {
                self.default(j, entry)
            } else {
                self.value(j, entry)
            }
        };
        // A group given one of its special values (`SIZE(*NOMAX)`) is that.
        if !kids.is_empty()
            && let Some(own) = self.special_given(i, entry, defaults)
        {
            return own;
        }
        match kids.first().map(|k| k.role) {
            Some(Role::Qualifier) => {
                // Name first, then the library: written LIB/NAME.
                let parts: Vec<String> = kids.iter().map(|k| v(k.index)).collect();
                if parts.first().is_none_or(String::is_empty) {
                    return String::new();
                }
                let mut keep = parts.len();
                while keep > 1 {
                    let k = kids[keep - 1];
                    let given = &parts[keep - 1];
                    let default = k.default.as_deref().unwrap_or("");
                    if given.is_empty() || given.eq_ignore_ascii_case(default) {
                        keep -= 1;
                    } else {
                        break;
                    }
                }
                let mut shown: Vec<String> = parts[..keep]
                    .iter()
                    .map(|s| {
                        if s.is_empty() {
                            "*N".to_owned()
                        } else {
                            s.clone()
                        }
                    })
                    .collect();
                shown.reverse();
                shown.join("/")
            }
            Some(_) => {
                let parts: Vec<String> = kids
                    .iter()
                    .map(|k| self.render(k.index, entry, defaults))
                    .collect();
                if parts.iter().all(String::is_empty) {
                    return String::new();
                }
                let mut parts: Vec<String> = parts
                    .into_iter()
                    .map(|s| if s.is_empty() { "*N".to_owned() } else { s })
                    .collect();
                while parts.len() > 1 && parts.last().is_some_and(|s| s == "*N") {
                    parts.pop();
                }
                parts.join(" ")
            }
            None => {
                let value = v(i);
                if value.is_empty() {
                    String::new()
                } else {
                    quote(node, &value)
                }
            }
        }
    }

    /// The special value a group node `i` was given for itself in
    /// `entry`, if one was.
    fn special_given(&self, i: u16, entry: usize, defaults: bool) -> Option<String> {
        let node = self.node(i)?;
        let own = if defaults {
            self.default(i, entry)
        } else {
            self.value(i, entry)
        };
        let own = own.trim();
        node.special_values
            .iter()
            .find(|s| s.eq_ignore_ascii_case(own))
            .cloned()
    }

    /// Takes the values in a typed command (`CMD KWD(value) ...`) for the
    /// parameters the definition has; others are left at their defaults.
    pub fn fill_from(&mut self, typed: &str) {
        for (kwd, value) in split_parameters(typed) {
            let Some(p) = self
                .parameters()
                .find(|p| p.keyword.eq_ignore_ascii_case(&kwd))
                .cloned()
            else {
                continue;
            };
            let kids: Vec<ParmNode> = self.children(p.index).cloned().collect();
            let wrap = kids.first().is_some_and(|k| k.role == Role::Element) && p.max_values > 1;
            let entries: Vec<String> = if p.max_values > 1 {
                words(&value)
                    .into_iter()
                    .map(|w| {
                        if wrap {
                            w.trim_start_matches('(').trim_end_matches(')').to_owned()
                        } else {
                            w
                        }
                    })
                    .collect()
            } else {
                vec![value.clone()]
            };
            for (e, text) in entries
                .iter()
                .enumerate()
                .take(usize::from(p.max_values.max(1)))
            {
                if e > 0 {
                    self.entries.insert(p.index, e + 1);
                }
                let special = p
                    .special_values
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(text));
                match kids.first().map(|k| k.role) {
                    _ if special && !kids.is_empty() => self.set(p.index, e, text),
                    Some(Role::Qualifier) => {
                        let mut parts: Vec<&str> = text.split('/').collect();
                        parts.reverse();
                        for (k, part) in kids.iter().zip(parts) {
                            self.set(k.index, e, part);
                        }
                    }
                    Some(_) => {
                        for (k, part) in kids.iter().zip(words(text)) {
                            self.set(k.index, e, &part);
                        }
                    }
                    None => self.set(p.index, e, text),
                }
            }
        }
    }
}

/// A value as CL takes it: quoted when it must be.
fn quote(node: &ParmNode, value: &str) -> String {
    let v = value.trim();
    if v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2 {
        return v.to_owned();
    }
    let special = v.starts_with('*')
        && (node
            .special_values
            .iter()
            .any(|s| s.eq_ignore_ascii_case(v))
            || node.values.iter().any(|s| s.eq_ignore_ascii_case(v)));
    let free = matches!(
        node.kind,
        ParmKind::Character | ParmKind::Path | ParmKind::Command
    );
    let plain = !v.is_empty()
        && v.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b"$#@_.*/".contains(&b));
    if special || !free || plain {
        v.to_owned()
    } else {
        format!("'{}'", v.replace('\'', "''"))
    }
}

/// A typed command's `KEYWORD(value)` parameters, values as typed (quotes
/// and inner parentheses kept). The command name and any value given
/// without a keyword are skipped.
pub fn split_parameters(typed: &str) -> Vec<(String, String)> {
    let b = typed.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'(' {
            i += 1;
            continue;
        }
        let mut k = i;
        while k > 0 && (b[k - 1].is_ascii_alphanumeric() || b[k - 1] == b'_') {
            k -= 1;
        }
        let kwd = typed[k..i].to_ascii_uppercase();
        let (mut depth, mut quoted, mut j) = (0usize, false, i);
        while j < b.len() {
            match b[j] {
                b'\'' => quoted = !quoted,
                b'(' if !quoted => depth += 1,
                b')' if !quoted => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if !kwd.is_empty() {
            out.push((kwd, typed[i + 1..j.min(b.len())].trim().to_owned()));
        }
        i = j + 1;
    }
    out
}

/// A value's blank-separated words; quoted strings and parenthesized
/// groups stay whole.
fn words(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut depth, mut quoted) = (0usize, false);
    for c in value.chars() {
        match c {
            '\'' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            ' ' if !quoted && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use qapd::testing::commands::{definition, find};

    use super::*;

    fn form(name: &str) -> Form {
        Form::new(definition(find(name).unwrap()))
    }

    fn param(f: &Form, kwd: &str) -> u16 {
        f.parameters().find(|p| p.keyword == kwd).unwrap().index
    }

    #[test]
    fn defaults_alone_give_the_bare_command() {
        for name in ["CRTLIB", "DSPOBJD", "CRTPF", "DLYJOB"] {
            assert_eq!(form(name).command_string(), name);
        }
    }

    #[test]
    fn crtlib_listed_values_and_quoting() {
        let mut f = form("CRTLIB");
        f.set(param(&f, "LIB"), 0, "QATESTLIB");
        assert_eq!(f.command_string(), "CRTLIB LIB(QATESTLIB)");
        // The default, given again, is left out.
        f.set(param(&f, "TYPE"), 0, "*PROD");
        assert_eq!(f.command_string(), "CRTLIB LIB(QATESTLIB)");
        f.set(param(&f, "TYPE"), 0, "*TEST");
        f.set(param(&f, "TEXT"), 0, "Bob's test library");
        assert_eq!(
            f.command_string(),
            "CRTLIB LIB(QATESTLIB) TYPE(*TEST) TEXT('Bob''s test library')"
        );
        f.set(param(&f, "TEXT"), 0, "'Already quoted'");
        assert!(f.command_string().ends_with("TEXT('Already quoted')"));
        f.set(param(&f, "TEXT"), 0, "*BLANK");
        assert_eq!(f.command_string(), "CRTLIB LIB(QATESTLIB) TYPE(*TEST)");
    }

    #[test]
    fn dspobjd_qualified_name_and_a_list() {
        let mut f = form("DSPOBJD");
        let obj = param(&f, "OBJ");
        let [name, lib]: [u16; 2] = f
            .children(obj)
            .map(|n| n.index)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        f.set(name, 0, "SECRET");
        // *LIBL is the library's default: left out.
        assert_eq!(f.command_string(), "DSPOBJD OBJ(SECRET)");
        f.set(lib, 0, "QSYS");
        let ty = param(&f, "OBJTYPE");
        f.set(ty, 0, "*FILE");
        assert!(f.can_add(ty));
        assert!(f.add_entry(ty));
        f.set(ty, 1, "*PGM");
        assert_eq!(
            f.command_string(),
            "DSPOBJD OBJ(QSYS/SECRET) OBJTYPE(*FILE *PGM)"
        );
        // Up to max_values entries and no more.
        for _ in 2..10 {
            assert!(f.add_entry(ty));
        }
        assert!(!f.add_entry(ty));
        assert_eq!(f.entries(ty), 10);
        // Entries left empty are not written.
        assert_eq!(
            f.command_string(),
            "DSPOBJD OBJ(QSYS/SECRET) OBJTYPE(*FILE *PGM)"
        );
    }

    #[test]
    fn crtpf_elements_under_additional_parameters() {
        let mut f = form("CRTPF");
        let file = param(&f, "FILE");
        let name = f.children(file).next().unwrap().index;
        f.set(name, 0, "ORDERS");
        let size = param(&f, "SIZE");
        assert_eq!(f.node(size).unwrap().show, Show::Additional);
        let el: Vec<u16> = f.children(size).map(|n| n.index).collect();
        assert_eq!(el.len(), 3);
        assert_eq!(f.command_string(), "CRTPF FILE(ORDERS)");
        f.set(el[0], 0, "20000");
        assert_eq!(f.command_string(), "CRTPF FILE(ORDERS) SIZE(20000 1000 3)");
        f.set(el[2], 0, "10");
        assert_eq!(f.command_string(), "CRTPF FILE(ORDERS) SIZE(20000 1000 10)");
        f.set(el[0], 0, "10000");
        f.set(el[2], 0, "3");
        assert_eq!(f.command_string(), "CRTPF FILE(ORDERS)");
    }

    #[test]
    fn dlyjob_an_integer() {
        let mut f = form("DLYJOB");
        f.set(param(&f, "DLY"), 0, "60");
        assert_eq!(f.command_string(), "DLYJOB DLY(60)");
    }

    #[test]
    fn a_typed_command_fills_the_form() {
        let mut f = form("DSPOBJD");
        f.fill_from("dspobjd obj(QGPL/ORDERS) objtype(*FILE *PGM)");
        assert_eq!(
            f.command_string(),
            "DSPOBJD OBJ(QGPL/ORDERS) OBJTYPE(*FILE *PGM)"
        );
        assert_eq!(f.entries(param(&f, "OBJTYPE")), 2);
        let mut f = form("CRTLIB");
        f.fill_from("CRTLIB LIB(X) TEXT('A (b) c') NOSUCH(1)");
        assert_eq!(f.command_string(), "CRTLIB LIB(X) TEXT('A (b) c')");
        let mut f = form("CRTPF");
        f.fill_from("CRTPF FILE(LIB1/F) SIZE(*NOMAX)");
        assert_eq!(f.command_string(), "CRTPF FILE(LIB1/F) SIZE(*NOMAX)");
    }

    #[test]
    fn words_and_parameters() {
        assert_eq!(words("*FILE  'a b' (c d)"), ["*FILE", "'a b'", "(c d)"]);
        assert_eq!(
            split_parameters("X A(1) B('x)') C((a b) (c))"),
            [
                ("A".to_owned(), "1".to_owned()),
                ("B".to_owned(), "'x)'".to_owned()),
                ("C".to_owned(), "(a b) (c)".to_owned()),
            ]
        );
    }
}
