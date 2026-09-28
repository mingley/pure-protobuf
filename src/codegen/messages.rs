//! MX-01 split of `super`: messages (mechanical move, no behavior change).

use super::*;
use crate::dynamic::{Cardinality, FieldDescriptor, FieldType, MessageDescriptor};

pub(crate) fn emit_zeroed_fields_struct<'a>(
    src: &mut String,
    name: &str,
    fields: impl Iterator<Item = &'a FieldDescriptor>,
) {
    let _ = writeln!(src, "#[derive(Clone, Debug, PartialEq)]");
    let _ = writeln!(src, "struct {name} {{");
    for f in fields {
        let _ = writeln!(src, "    {}: {},", field_id(f), field_storage_ty(f));
    }
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl Default for {name} {{");
    let _ = writeln!(src, "    #[inline(always)]");
    let _ = writeln!(
        src,
        "    fn default() -> Self {{ unsafe {{ pbrs::rt::zeroed_message() }} }}"
    );
    let _ = writeln!(src, "}}");
}

pub(crate) fn emit_box_eq(src: &mut String, field: &str, ty: &str) {
    let _ = writeln!(
        src,
        "        match (self.{field}.as_deref(), other.{field}.as_deref()) {{"
    );
    let _ = writeln!(src, "            (None, None) => {{}}");
    let _ = writeln!(
        src,
        "            (Some(a), Some(b)) => if a != b {{ return false; }}"
    );
    let _ = writeln!(
        src,
        "            (None, Some(b)) => if *b != {ty}::default() {{ return false; }}"
    );
    let _ = writeln!(
        src,
        "            (Some(a), None) => if *a != {ty}::default() {{ return false; }}"
    );
    let _ = writeln!(src, "        }}");
}

pub(crate) fn packed_storage_ty(f: &FieldDescriptor) -> &'static str {
    match f.field_type {
        FieldType::Int32 | FieldType::Enum => "pbrs::rt::PackedI32",
        FieldType::Int64 => "pbrs::rt::PackedI64",
        FieldType::Uint32 => "pbrs::rt::PackedU32",
        FieldType::Uint64 => "pbrs::rt::PackedU64",
        FieldType::Sint32 => "pbrs::rt::PackedS32",
        FieldType::Sint64 => "pbrs::rt::PackedS64",
        FieldType::Fixed32 => "pbrs::rt::PackedFx32",
        FieldType::Sfixed32 => "pbrs::rt::PackedSfx32",
        FieldType::Fixed64 => "pbrs::rt::PackedFx64",
        FieldType::Sfixed64 => "pbrs::rt::PackedSfx64",
        FieldType::Float => "pbrs::rt::PackedF32",
        FieldType::Double => "pbrs::rt::PackedF64",
        FieldType::Bool => "pbrs::rt::PackedBool",
        _ => "pbrs::rt::PackedI32",
    }
}

pub(crate) fn field_storage_ty(f: &FieldDescriptor) -> String {
    if f.is_map {
        let (k, v) = map_kv(f);
        return format!("Map<{k}, {v}>");
    }
    let t = scalar_type(f);
    if is_packed_scalar(f) {
        packed_storage_ty(f).into()
    } else if f.cardinality == Cardinality::Repeated {
        format!("Repeated<{t}>")
    } else if is_lazy_msg(f) {
        format!("pbrs::rt::LazyMsg<{t}>")
    } else if f.field_type == FieldType::Message || f.field_type == FieldType::Group {
        format!("Option<Box<{t}>>")
    } else if is_option(f) {
        match f.field_type {
            FieldType::Bool => "pbrs::rt::OptBool".into(),
            FieldType::String => "Option<Box<pbrs::rt::LazyStr>>".into(),
            FieldType::Bytes => "Option<Box<pbrs::rt::LazyBytes>>".into(),
            _ => format!("Option<{t}>"),
        }
    } else {
        t
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BacktickRun {
    start: usize,
    len: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CodeSpan {
    start: usize,
    end: usize,
}

pub(crate) fn sanitize_doc_line(line: &str) -> String {
    let trimmed = line.trim_start_matches(' ');
    let leading_spaces = line.len() - trimmed.len();
    let prefix = if leading_spaces >= 4 {
        "  "
    } else {
        &line[..leading_spaces]
    };
    let s = trimmed;
    if s.is_empty() {
        return prefix.to_string();
    }

    let b = s.as_bytes();
    let mut runs = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            let mut bs_count = 0;
            let mut k = i;
            while k > 0 && b[k - 1] == b'\\' {
                bs_count += 1;
                k -= 1;
            }
            if bs_count % 2 == 0 {
                let start = i;
                while i < b.len() && b[i] == b'`' {
                    i += 1;
                }
                runs.push(BacktickRun {
                    start,
                    len: i - start,
                });
                continue;
            }
        }
        i += 1;
    }

    let mut code_spans = Vec::new();
    let mut unclosed_backtick_starts = std::collections::BTreeSet::new();
    let mut run_idx = 0;
    while run_idx < runs.len() {
        let open = runs[run_idx];
        let mut matched = false;
        for (j, close) in runs.iter().enumerate().skip(run_idx + 1) {
            if close.len == open.len {
                code_spans.push(CodeSpan {
                    start: open.start,
                    end: close.start + close.len,
                });
                run_idx = j + 1;
                matched = true;
                break;
            }
        }
        if !matched {
            for offset in 0..open.len {
                unclosed_backtick_starts.insert(open.start + offset);
            }
            run_idx += 1;
        }
    }

    let mut out = String::with_capacity(s.len() + 16);
    out.push_str(prefix);

    let mut cur = 0;
    let mut span_idx = 0;

    while cur < s.len() {
        if span_idx < code_spans.len() && cur == code_spans[span_idx].start {
            let end = code_spans[span_idx].end;
            out.push_str(&s[cur..end]);
            cur = end;
            span_idx += 1;
        } else {
            let seg_end = if span_idx < code_spans.len() {
                code_spans[span_idx].start
            } else {
                s.len()
            };

            while cur < seg_end {
                let rest = &s[cur..seg_end];

                if unclosed_backtick_starts.contains(&cur) {
                    out.push('\\');
                    out.push('`');
                    cur += 1;
                    continue;
                }

                if rest.starts_with('\\') && rest.len() > 1 {
                    let next_b = rest.as_bytes()[1];
                    if matches!(next_b, b'[' | b']' | b'<' | b'>' | b'`' | b'\\') {
                        out.push('\\');
                        out.push(next_b as char);
                        cur += 2;
                        continue;
                    }
                }

                if rest.starts_with('<') {
                    if let Some(gt) = rest.find('>') {
                        let inner = &rest[1..gt];
                        if inner.starts_with("http://")
                            || inner.starts_with("https://")
                            || inner.starts_with("mailto:")
                        {
                            out.push_str(&rest[..=gt]);
                            cur += gt + 1;
                            continue;
                        }
                    }
                    out.push_str("\\<");
                    cur += 1;
                    continue;
                }

                if rest.starts_with('>') {
                    out.push_str("\\>");
                    cur += 1;
                    continue;
                }

                if rest.starts_with('[') {
                    let mut is_link = false;
                    if let Some(rb) = rest.find(']') {
                        let after_rb = &rest[rb + 1..];
                        if after_rb.starts_with('(') {
                            if let Some(rp) = after_rb.find(')') {
                                let url = &after_rb[1..rp];
                                if url.starts_with("http://")
                                    || url.starts_with("https://")
                                    || url.starts_with("mailto:")
                                {
                                    let link_len = rb + 1 + rp + 1;
                                    out.push_str(&rest[..link_len]);
                                    cur += link_len;
                                    is_link = true;
                                }
                            }
                        }
                    }
                    if is_link {
                        continue;
                    }
                    out.push_str("\\[");
                    cur += 1;
                    continue;
                }

                if rest.starts_with(']') {
                    out.push_str("\\]");
                    cur += 1;
                    continue;
                }

                if rest.starts_with("http://") || rest.starts_with("https://") {
                    let url_len = rest
                        .find(|c: char| {
                            c.is_whitespace()
                                || matches!(c, '<' | '>' | '[' | ']' | '(' | ')' | '"' | '\'' | '`')
                        })
                        .unwrap_or(rest.len());
                    let raw_url = &rest[..url_len];
                    let trimmed_url = raw_url.trim_end_matches(['.', ',', ';', ':', '!', '?']);
                    let trail = &raw_url[trimmed_url.len()..];

                    if !trimmed_url.is_empty() {
                        out.push('<');
                        out.push_str(trimmed_url);
                        out.push('>');
                        out.push_str(trail);
                        cur += raw_url.len();
                        continue;
                    }
                }

                if let Some(ch) = rest.chars().next() {
                    out.push(ch);
                    cur += ch.len_utf8();
                } else {
                    break;
                }
            }
        }
    }

    out
}

pub(crate) fn emit_doc_line(src: &mut String, line: &str, indent: &str, in_code_fence: &mut bool) {
    let trimmed_start = line.trim_start();
    if trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~") {
        if !*in_code_fence {
            *in_code_fence = true;
            let _ = writeln!(src, "{indent}/// ```text");
        } else {
            *in_code_fence = false;
            let _ = writeln!(src, "{indent}/// ```");
        }
        return;
    }
    if *in_code_fence {
        if line.is_empty() {
            let _ = writeln!(src, "{indent}///");
        } else {
            let _ = writeln!(src, "{indent}/// {line}");
        }
        return;
    }
    let sanitized = sanitize_doc_line(line);
    if sanitized.is_empty() {
        let _ = writeln!(src, "{indent}///");
    } else {
        let _ = writeln!(src, "{indent}/// {sanitized}");
    }
}

pub(crate) fn emit_doc_comments(src: &mut String, comments: &Comments, indent: &str) {
    let mut in_code_fence = false;
    if let Some(leading) = comments.leading() {
        for line in leading.lines() {
            let trimmed = line.strip_prefix(' ').unwrap_or(line);
            emit_doc_line(src, trimmed, indent, &mut in_code_fence);
        }
        if in_code_fence {
            let _ = writeln!(src, "{indent}/// ```");
            in_code_fence = false;
        }
    }
    if let Some(trailing) = comments.trailing() {
        for line in trailing.lines() {
            let trimmed = line.strip_prefix(' ').unwrap_or(line);
            emit_doc_line(src, trimmed, indent, &mut in_code_fence);
        }
        if in_code_fence {
            let _ = writeln!(src, "{indent}/// ```");
        }
    }
}

pub(crate) fn emit_field_getter_doc(
    src: &mut String,
    desc: &MessageDescriptor,
    f: &FieldDescriptor,
) {
    emit_doc_comments(src, &f.comments, "    ");
    if !f.comments.is_empty() {
        let _ = writeln!(src, "    ///");
    }
    let _ = writeln!(src, "    /// Field `{}` (number {}).", f.name, f.number);
    let _ = writeln!(src, "    ///");
    if f.is_map {
        let (k, v) = map_kv(f);
        let _ = writeln!(
            src,
            "    /// Map field with key `{k}` and value `{v}`. Empty by default."
        );
    } else if f.cardinality == Cardinality::Repeated {
        let t = scalar_type(f);
        let packed_note = if is_packed_scalar(f) {
            " (wire format: packed)"
        } else {
            ""
        };
        let _ = writeln!(
            src,
            "    /// Repeated field of `{t}`{packed_note}. Empty by default."
        );
    } else if f.field_type == FieldType::Message || f.field_type == FieldType::Group {
        let _ = writeln!(
            src,
            "    /// Message field with explicit presence. Returns a reference to the message, or the default instance if unset."
        );
    } else if f.field_type == FieldType::String {
        let def = f.default.as_deref().unwrap_or("");
        if is_option(f) {
            let _ = writeln!(
                src,
                "    /// Explicit optional string. Returns the string value, or the default (\"{}\") if unset.",
                def.escape_default()
            );
        } else {
            let _ = writeln!(
                src,
                "    /// Implicit presence string (default: \"{}\").",
                def.escape_default()
            );
        }
    } else if f.field_type == FieldType::Bytes {
        if is_option(f) {
            let _ = writeln!(
                src,
                "    /// Explicit optional bytes. Returns the byte slice, or the default if unset."
            );
        } else {
            let _ = writeln!(src, "    /// Implicit presence bytes (empty by default).");
        }
    } else {
        let def = scalar_default_expr(f);
        if is_option(f) {
            if f.cardinality == Cardinality::Required {
                let _ = writeln!(
                    src,
                    "    /// Required field. Returns the value of `{}`.",
                    f.name
                );
            } else {
                let _ = writeln!(
                    src,
                    "    /// Explicit optional field. Returns the value of `{}` or the default (`{def}`) if unset.",
                    f.name
                );
            }
        } else {
            let _ = writeln!(src, "    /// Implicit presence field (default: `{def}`).");
        }
    }
    if is_real_oneof(desc, f) {
        let _ = writeln!(
            src,
            "    /// Part of a oneof: setting this field clears other fields in the oneof."
        );
    }
    if f.deprecated {
        let _ = writeln!(src, "    ///");
        let _ = writeln!(src, "    /// # Deprecated");
        let _ = writeln!(src, "    #[deprecated]");
    }
}

pub(crate) fn emit_has_doc(src: &mut String, f: &FieldDescriptor) {
    let _ = writeln!(src, "    /// Returns `true` if field `{}` is set.", f.name);
    if f.deprecated {
        let _ = writeln!(src, "    #[deprecated]");
    }
}

pub(crate) fn emit_opt_doc(src: &mut String, f: &FieldDescriptor) {
    let _ = writeln!(
        src,
        "    /// Returns `Some` if field `{}` is set, or `None` if unset.",
        f.name
    );
    if f.deprecated {
        let _ = writeln!(src, "    #[deprecated]");
    }
}

pub(crate) fn emit_mut_doc(src: &mut String, f: &FieldDescriptor, is_msg: bool) {
    if is_msg {
        let _ = writeln!(
            src,
            "    /// Returns a mutable reference to `{}`, initializing it with default values if unset.",
            f.name
        );
    } else {
        let _ = writeln!(src, "    /// Returns a mutable view of field `{}`.", f.name);
    }
    if f.deprecated {
        let _ = writeln!(src, "    #[deprecated]");
    }
}

pub(crate) fn emit_view_doc(src: &mut String, f: &FieldDescriptor) {
    let _ = writeln!(src, "    /// Returns a view of field `{}`.", f.name);
    if f.deprecated {
        let _ = writeln!(src, "    #[deprecated]");
    }
}

pub(crate) fn emit_set_doc(src: &mut String, desc: &MessageDescriptor, f: &FieldDescriptor) {
    let _ = writeln!(src, "    /// Sets the value of `{}`.", f.name);
    if is_real_oneof(desc, f) {
        let _ = writeln!(
            src,
            "    /// Part of a oneof: clears any other set field in the oneof."
        );
    }
    if f.deprecated {
        let _ = writeln!(src, "    #[deprecated]");
    }
}

pub(crate) fn emit_clear_doc(src: &mut String, f: &FieldDescriptor) {
    let _ = writeln!(src, "    /// Clears field `{}`, marking it unset.", f.name);
    if f.deprecated {
        let _ = writeln!(src, "    #[deprecated]");
    }
}

pub(crate) fn emit_message(src: &mut String, desc: &MessageDescriptor, edition2024: bool) {
    bind_field_idents(desc);
    let name = rust_ident(&desc.full_name);
    let view = format!("{name}View");
    let mut_ = format!("{name}Mut");
    let cold_name = format!("{name}Cold");
    let use_cold = uses_cold_storage(desc);
    if use_cold {
        emit_zeroed_fields_struct(
            src,
            &cold_name,
            desc.fields.values().filter(|f| stored_cold(desc, f)),
        );
    }
    emit_doc_comments(src, &desc.comments, "");
    if !desc.comments.is_empty() {
        let _ = writeln!(src, "///");
    }
    let _ = writeln!(src, "/// The `{}` protobuf message.", desc.full_name);
    if desc.deprecated {
        let _ = writeln!(src, "///");
        let _ = writeln!(src, "/// # Deprecated");
        let _ = writeln!(src, "#[deprecated]");
    }
    let _ = writeln!(src, "#[derive(Clone, Debug)]");
    let _ = writeln!(src, "pub struct {name} {{");
    for f in desc.fields.values().filter(|f| stored_hot(desc, f)) {
        emit_doc_comments(src, &f.comments, "    ");
        let _ = writeln!(src, "    {}: {},", field_id(f), field_storage_ty(f));
    }
    if use_cold {
        let _ = writeln!(src, "    cold: Option<Box<{cold_name}>>,");
    }
    let _ = writeln!(src, "    unknown: UnknownFields,");
    let _ = writeln!(src, "    cached_size: pbrs::rt::CachedSize,");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl PartialEq for {name} {{");
    let _ = writeln!(src, "    fn eq(&self, other: &Self) -> bool {{");
    for f in desc.fields.values().filter(|f| stored_hot(desc, f)) {
        let id = field_id(f);
        let _ = writeln!(
            src,
            "        if self.{id} != other.{id} {{ return false; }}"
        );
    }
    if use_cold {
        emit_box_eq(src, "cold", &cold_name);
    }
    let _ = writeln!(src, "        self.unknown == other.unknown");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl Eq for {name} {{}}");
    let _ = writeln!(src, "impl Default for {name} {{");
    let _ = writeln!(src, "    #[inline(always)]");
    let _ = writeln!(src, "    fn default() -> Self {{");
    let _ = writeln!(src, "        unsafe {{ pbrs::rt::zeroed_message() }}");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "}}");

    let _ = writeln!(src, "impl {name} {{");
    let _ = writeln!(
        src,
        "    /// Creates a new, default instance of [`{name}`]."
    );
    let _ = writeln!(src, "    pub fn new() -> Self {{ Self::default() }}");
    if use_cold {
        let _ = writeln!(
            src,
            "    #[inline(always)] fn cold_mut(&mut self) -> &mut {cold_name} {{ self.cold.get_or_insert_with(|| Box::new({cold_name}::default())) }}"
        );
    }
    let empty_ok = !desc
        .fields
        .values()
        .any(|f| f.cardinality == Cardinality::Required);
    let _ = writeln!(
        src,
        "    /// Whether an empty byte slice is a valid encoding of this message."
    );
    let _ = writeln!(src, "    pub const EMPTY_PARSE_OK: bool = {empty_ok};");
    let _ = writeln!(
        src,
        "    /// The fully-qualified protobuf name of this message."
    );
    let _ = writeln!(
        src,
        "    pub const FULL_NAME: &'static str = \"{}\";",
        desc.full_name
    );
    for f in desc.fields.values() {
        emit_accessors(src, desc, f);
    }
    emit_codec(src, desc, edition2024);
    if std::env::var("PURE_PROTOBUF_NO_REFLECT").as_deref() != Ok("1") {
        emit_json_text(src, desc);
    }
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "pbrs::impl_typed_message!({name}, {view}, {mut_});");
}

pub(crate) fn emit_oneof_clear(src: &mut String, desc: &MessageDescriptor, f: &FieldDescriptor) {
    let Some(idx) = f.oneof_index else {
        return;
    };
    let Some(members) = desc.oneofs.get(idx as usize) else {
        return;
    };
    let mut sorted_members = members.clone();
    sorted_members.sort();
    for n in &sorted_members {
        if *n == f.number {
            continue;
        }
        if let Some(sib) = desc.field(*n) {
            let st = store_mut(desc, sib);
            if is_lazy_msg(sib) {
                let _ = writeln!(src, "        {st} = Default::default();");
            } else if is_option(sib) && sib.field_type == FieldType::Bool {
                let _ = writeln!(src, "        {st} = pbrs::rt::OptBool::NONE;");
            } else if is_option(sib) {
                let _ = writeln!(src, "        {st} = None;");
            } else if sib.cardinality == Cardinality::Repeated || sib.is_map {
                let _ = writeln!(src, "        {st}.clear();");
            } else {
                let _ = writeln!(src, "        {st} = Default::default();");
            }
        }
    }
}

pub(crate) fn emit_accessors(src: &mut String, desc: &MessageDescriptor, f: &FieldDescriptor) {
    let id = field_id(f);
    let m = field_raw(f);
    if f.is_map {
        let (k, v) = map_kv(f);
        let st = store_mut(desc, f);
        emit_field_getter_doc(src, desc, f);
        if stored_cold(desc, f) {
            let _ = writeln!(
                src,
                "    pub fn {id}(&self) -> MapView<'_, {k}, {v}> {{ self.cold.as_ref().map(|c| c.{id}.as_view()).unwrap_or_else(MapView::empty) }}"
            );
        } else {
            let _ = writeln!(
                src,
                "    pub fn {id}(&self) -> MapView<'_, {k}, {v}> {{ self.{id}.as_view() }}"
            );
        }
        emit_mut_doc(src, f, false);
        let _ = writeln!(
            src,
            "    pub fn {id}_mut(&mut self) -> MapMut<'_, {k}, {v}> {{ self.cached_size.dirty(); {st}.as_mut() }}"
        );
        emit_set_doc(src, desc, f);
        let _ = writeln!(
            src,
            "    pub fn set_{m}(&mut self, v: Map<{k}, {v}>) {{ self.cached_size.dirty(); {st} = v; }}"
        );
        return;
    }
    if f.cardinality == Cardinality::Repeated {
        let t = scalar_type(f);
        let st = store_mut(desc, f);
        emit_field_getter_doc(src, desc, f);
        if stored_cold(desc, f) {
            let _ = writeln!(
                src,
                "    pub fn {id}(&self) -> RepeatedView<'_, {t}> {{ self.cold.as_ref().map(|c| c.{id}.as_view()).unwrap_or_else(|| RepeatedView::from_slice(&[])) }}"
            );
        } else {
            let _ = writeln!(
                src,
                "    pub fn {id}(&self) -> RepeatedView<'_, {t}> {{ self.{id}.as_view() }}"
            );
        }
        emit_mut_doc(src, f, false);
        let _ = writeln!(
            src,
            "    pub fn {id}_mut(&mut self) -> RepeatedMut<'_, {t}> {{ self.cached_size.dirty(); {st}.as_mut() }}"
        );
        emit_set_doc(src, desc, f);
        if is_packed_scalar(f) {
            let _ = writeln!(
                src,
                "    pub fn set_{m}(&mut self, v: impl IntoIterator<Item = {t}>) {{ self.cached_size.dirty(); {st} = pbrs::rt::Packed::from_repeated(v.into_iter().collect()); }}"
            );
        } else {
            let _ = writeln!(
                src,
                "    pub fn set_{m}(&mut self, v: impl IntoIterator<Item = {t}>) {{ self.cached_size.dirty(); {st} = v.into_iter().collect(); }}"
            );
        }
        return;
    }
    if f.field_type == FieldType::Message || f.field_type == FieldType::Group {
        let t = scalar_type(f);
        let st = store_mut(desc, f);
        if stored_cold(desc, f) {
            emit_has_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn has_{m}(&self) -> bool {{ self.cold.as_ref().is_some_and(|c| c.{id}.is_some()) }}"
            );
            emit_field_getter_doc(src, desc, f);
            let _ = writeln!(
                src,
                "    pub fn {id}(&self) -> &{t} {{ self.cold.as_ref().and_then(|c| c.{id}.as_deref()).unwrap_or(pbrs::gen_support::default_instance_of()) }}"
            );
            emit_opt_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn {id}_opt(&self) -> Option<&{t}> {{ self.cold.as_ref().and_then(|c| c.{id}.as_deref()) }}"
            );
        } else {
            emit_has_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn has_{m}(&self) -> bool {{ self.{id}.is_some() }}"
            );
            emit_field_getter_doc(src, desc, f);
            let _ = writeln!(
                src,
                "    pub fn {id}(&self) -> &{t} {{ self.{id}.as_deref().unwrap_or(pbrs::gen_support::default_instance_of()) }}"
            );
            emit_opt_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn {id}_opt(&self) -> Option<&{t}> {{ self.{id}.as_deref() }}"
            );
        }
        emit_view_doc(src, f);
        let _ = writeln!(
            src,
            "    pub fn {id}_view(&self) -> {t}View<'_> {{ {t}View(self.{id}()) }}"
        );
        emit_set_doc(src, desc, f);
        let _ = writeln!(src, "    pub fn set_{m}(&mut self, v: {t}) {{");
        let _ = writeln!(src, "        self.cached_size.dirty();");
        emit_oneof_clear(src, desc, f);
        if is_lazy_msg(f) {
            let _ = writeln!(src, "        {st} = pbrs::rt::LazyMsg::from_owned(v);");
        } else {
            let _ = writeln!(src, "        {st} = Some(Box::new(v));");
        }
        let _ = writeln!(src, "    }}");
        emit_mut_doc(src, f, true);
        if is_lazy_msg(f) {
            let _ = writeln!(
                src,
                "    pub fn {id}_mut(&mut self) -> &mut {t} {{ self.cached_size.dirty(); {st}.get_or_insert() }}"
            );
            emit_clear_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn clear_{m}(&mut self) {{ self.cached_size.dirty(); {st}.clear(); }}"
            );
        } else {
            let _ = writeln!(
                src,
                "    pub fn {id}_mut(&mut self) -> &mut {t} {{ self.cached_size.dirty(); {st}.get_or_insert_with(|| Box::new({t}::default())).as_mut() }}"
            );
            emit_clear_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn clear_{m}(&mut self) {{ self.cached_size.dirty(); {st} = None; }}"
            );
        }
        return;
    }
    if f.field_type == FieldType::String {
        let fallback = string_default_lit(f);
        let st = store_mut(desc, f);
        let read = if stored_cold(desc, f) {
            format!("self.cold.as_ref().and_then(|c| c.{id}.as_ref())")
        } else {
            format!("self.{id}.as_ref()")
        };
        let read_view = if stored_cold(desc, f) {
            format!("self.cold.as_ref().map(|c| c.{id}.as_view())")
        } else {
            format!("Some(self.{id}.as_view())")
        };
        if is_option(f) {
            emit_has_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn has_{m}(&self) -> bool {{ {read}.is_some() }}"
            );
            emit_field_getter_doc(src, desc, f);
            let _ = writeln!(
                src,
                "    pub fn {id}(&self) -> &pbrs::ProtoStr {{ {read}.map(|s| s.as_view()).unwrap_or_else(|| pbrs::ProtoStr::from_bytes({fallback})) }}"
            );
            emit_opt_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn {id}_opt(&self) -> Option<&pbrs::ProtoStr> {{ {read}.map(|s| s.as_view()) }}"
            );
            emit_set_doc(src, desc, f);
            let _ = writeln!(
                src,
                "    pub fn set_{m}(&mut self, v: impl pbrs::IntoProxied<ProtoString>) {{"
            );
            let _ = writeln!(src, "        self.cached_size.dirty();");
            emit_oneof_clear(src, desc, f);
            let _ = writeln!(
                src,
                "        {st} = Some(Box::new(pbrs::rt::LazyStr::owned(v.into_proxied(pbrs::__internal::Private))));"
            );
            let _ = writeln!(src, "    }}");
            emit_clear_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn clear_{m}(&mut self) {{ self.cached_size.dirty(); {st} = None; }}"
            );
        } else {
            emit_field_getter_doc(src, desc, f);
            let _ = writeln!(
                src,
                "    pub fn {id}(&self) -> &pbrs::ProtoStr {{ {read_view}.unwrap_or_else(|| pbrs::ProtoStr::from_bytes({fallback})) }}"
            );
            emit_set_doc(src, desc, f);
            let _ = writeln!(
                src,
                "    pub fn set_{m}(&mut self, v: impl pbrs::IntoProxied<ProtoString>) {{ self.cached_size.dirty(); {st} = pbrs::rt::LazyStr::owned(v.into_proxied(pbrs::__internal::Private)); }}"
            );
        }
        return;
    }
    if f.field_type == FieldType::Bytes {
        let fallback = bytes_default_lit(f);
        let st = store_mut(desc, f);
        let read = if stored_cold(desc, f) {
            format!("self.cold.as_ref().and_then(|c| c.{id}.as_ref())")
        } else {
            format!("self.{id}.as_ref()")
        };
        let read_bytes = if stored_cold(desc, f) {
            format!("self.cold.as_ref().map(|c| c.{id}.as_bytes())")
        } else {
            format!("Some(self.{id}.as_bytes())")
        };
        if is_option(f) {
            emit_has_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn has_{m}(&self) -> bool {{ {read}.is_some() }}"
            );
            emit_field_getter_doc(src, desc, f);
            let _ = writeln!(
                src,
                "    pub fn {id}(&self) -> &[u8] {{ {read}.map(|b| b.as_bytes()).unwrap_or({fallback}) }}"
            );
            emit_opt_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn {id}_opt(&self) -> Option<&[u8]> {{ {read}.map(|b| b.as_bytes()) }}"
            );
            emit_set_doc(src, desc, f);
            let _ = writeln!(
                src,
                "    pub fn set_{m}(&mut self, v: impl pbrs::IntoProxied<ProtoBytes>) {{ self.cached_size.dirty(); {st} = Some(Box::new(pbrs::rt::LazyBytes::owned(v.into_proxied(pbrs::__internal::Private)))); }}"
            );
            emit_clear_doc(src, f);
            let _ = writeln!(
                src,
                "    pub fn clear_{m}(&mut self) {{ self.cached_size.dirty(); {st} = None; }}"
            );
        } else {
            emit_field_getter_doc(src, desc, f);
            let _ = writeln!(
                src,
                "    pub fn {id}(&self) -> &[u8] {{ {read_bytes}.unwrap_or({fallback}) }}"
            );
            emit_set_doc(src, desc, f);
            let _ = writeln!(
                src,
                "    pub fn set_{m}(&mut self, v: impl pbrs::IntoProxied<ProtoBytes>) {{ self.cached_size.dirty(); {st} = pbrs::rt::LazyBytes::owned(v.into_proxied(pbrs::__internal::Private)); }}"
            );
        }
        return;
    }
    let t = scalar_type(f);
    let enum_ty = enum_api_ty(f);
    let get_ty = enum_ty.as_deref().unwrap_or(t.as_str());
    let wrap_open = enum_ty
        .as_ref()
        .map(|e| format!("{e}("))
        .unwrap_or_default();
    let wrap_close = if enum_ty.is_some() { ")" } else { "" };
    let def = scalar_default_expr(f);
    let set_ty = if enum_ty.is_some() {
        format!("impl Into<{t}>")
    } else {
        t.clone()
    };
    let set_val = if enum_ty.is_some() { "v.into()" } else { "v" };
    if is_option(f) && f.field_type == FieldType::Bool {
        emit_has_doc(src, f);
        let _ = writeln!(
            src,
            "    pub fn has_{m}(&self) -> bool {{ self.{id}.is_some() }}"
        );
        emit_field_getter_doc(src, desc, f);
        let _ = writeln!(
            src,
            "    pub fn {id}(&self) -> bool {{ self.{id}.unwrap_or({def}) }}"
        );
        emit_opt_doc(src, f);
        let _ = writeln!(
            src,
            "    pub fn {id}_opt(&self) -> Option<bool> {{ self.{id}.get() }}"
        );
        emit_set_doc(src, desc, f);
        let _ = writeln!(src, "    pub fn set_{m}(&mut self, v: bool) {{");
        let _ = writeln!(src, "        self.cached_size.dirty();");
        emit_oneof_clear(src, desc, f);
        let _ = writeln!(src, "        self.{id} = pbrs::rt::OptBool::some(v);");
        let _ = writeln!(src, "    }}");
        emit_clear_doc(src, f);
        let _ = writeln!(
            src,
            "    pub fn clear_{m}(&mut self) {{ self.cached_size.dirty(); self.{id} = pbrs::rt::OptBool::NONE; }}"
        );
    } else if is_option(f) {
        emit_has_doc(src, f);
        let _ = writeln!(
            src,
            "    pub fn has_{m}(&self) -> bool {{ self.{id}.is_some() }}"
        );
        emit_field_getter_doc(src, desc, f);
        let _ = writeln!(
            src,
            "    pub fn {id}(&self) -> {get_ty} {{ {wrap_open}self.{id}.unwrap_or({def}){wrap_close} }}"
        );
        emit_opt_doc(src, f);
        let _ = writeln!(
            src,
            "    pub fn {id}_opt(&self) -> Option<{get_ty}> {{ self.{id}.map(|v| {wrap_open}v{wrap_close}) }}"
        );
        emit_set_doc(src, desc, f);
        let _ = writeln!(src, "    pub fn set_{m}(&mut self, v: {set_ty}) {{");
        let _ = writeln!(src, "        self.cached_size.dirty();");
        emit_oneof_clear(src, desc, f);
        let _ = writeln!(src, "        self.{id} = Some({set_val});");
        let _ = writeln!(src, "    }}");
        emit_clear_doc(src, f);
        let _ = writeln!(
            src,
            "    pub fn clear_{m}(&mut self) {{ self.cached_size.dirty(); self.{id} = None; }}"
        );
    } else {
        emit_field_getter_doc(src, desc, f);
        let _ = writeln!(
            src,
            "    pub fn {id}(&self) -> {get_ty} {{ {wrap_open}self.{id}{wrap_close} }}"
        );
        emit_set_doc(src, desc, f);
        let _ = writeln!(
            src,
            "    pub fn set_{m}(&mut self, v: {set_ty}) {{ self.cached_size.dirty(); self.{id} = {set_val}; }}"
        );
    }
}

pub(crate) fn enum_api_ty(f: &FieldDescriptor) -> Option<String> {
    if f.field_type != FieldType::Enum {
        return None;
    }
    let name = f
        .enum_ty
        .as_ref()
        .map(|e| e.full_name.as_str())
        .or(f.type_name.as_deref())
        .unwrap_or("UnknownEnum");
    Some(rust_type_path(name))
}

pub(crate) fn enum_first_number(f: &FieldDescriptor) -> i32 {
    f.enum_ty
        .as_ref()
        .and_then(|e| e.listed.first().map(|x| x.0))
        .unwrap_or(0)
}

pub(crate) fn scalar_default_expr(f: &FieldDescriptor) -> String {
    if f.field_type == FieldType::Enum {
        if let Some(d) = f.default.as_deref() {
            if let Ok(n) = d.parse::<i32>() {
                return n.to_string();
            }
            if let Some(n) = f.enum_ty.as_ref().and_then(|e| e.names.get(d)) {
                return n.to_string();
            }
        }
        return enum_first_number(f).to_string();
    }
    let Some(d) = f.default.as_deref() else {
        return match f.field_type {
            FieldType::Bool => "false".into(),
            FieldType::Float | FieldType::Double => "0.0".into(),
            _ => "0".into(),
        };
    };
    match f.field_type {
        FieldType::Bool => {
            if d.eq_ignore_ascii_case("true") {
                "true".into()
            } else {
                "false".into()
            }
        }
        FieldType::Float | FieldType::Double => {
            let f32ty = f.field_type == FieldType::Float;
            let inf = if f32ty {
                "f32::INFINITY"
            } else {
                "f64::INFINITY"
            };
            let ninf = if f32ty {
                "f32::NEG_INFINITY"
            } else {
                "f64::NEG_INFINITY"
            };
            let nan = if f32ty { "f32::NAN" } else { "f64::NAN" };
            let lower = d.to_ascii_lowercase();
            if lower == "inf" || lower == "infinity" || lower == "+inf" || lower == "+infinity" {
                inf.into()
            } else if lower == "-inf" || lower == "-infinity" {
                ninf.into()
            } else if lower == "nan" {
                nan.into()
            } else if d.contains('.') || d.contains('e') || d.contains('E') {
                d.to_string()
            } else {
                format!("{d}.0")
            }
        }
        _ => d.to_string(),
    }
}

pub(crate) fn rust_byte_lit(bytes: &[u8]) -> String {
    let mut out = String::from("b\"");
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push('"');
    out
}

pub(crate) fn unescape_c_bytes(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            i += 1;
            match b[i] {
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'\\' => out.push(b'\\'),
                b'"' => out.push(b'"'),
                b'\'' => out.push(b'\''),
                b'x' if i + 2 < b.len() => {
                    let h = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("00");
                    out.push(u8::from_str_radix(h, 16).unwrap_or(0));
                    i += 2;
                }
                c if c.is_ascii_digit() => {
                    let mut v = 0u8;
                    let mut n = 0;
                    while i < b.len() && n < 3 && b[i].is_ascii_digit() {
                        v = v.saturating_mul(8).saturating_add(b[i] - b'0');
                        i += 1;
                        n += 1;
                    }
                    out.push(v);
                    continue;
                }
                c => out.push(c),
            }
            i += 1;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

pub(crate) fn string_default_lit(f: &FieldDescriptor) -> String {
    match f.default.as_deref() {
        Some(d) => rust_byte_lit(&unescape_c_bytes(d)),
        None => "b\"\"".into(),
    }
}

pub(crate) fn bytes_default_lit(f: &FieldDescriptor) -> String {
    string_default_lit(f)
}

pub(crate) fn emit_enum(src: &mut String, ed: &crate::dynamic::EnumDescriptor) {
    let name = rust_ident(&ed.full_name);
    let values = rust_enum_values(&ed.name, &ed.listed);
    let first = ed.listed.first().map(|x| x.0).unwrap_or(0);
    let known = values
        .iter()
        .map(|v| v.number.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    emit_doc_comments(src, &ed.comments, "");
    if !ed.comments.is_empty() {
        let _ = writeln!(src, "///");
    }
    let _ = writeln!(src, "/// The `{}` enum.", ed.full_name);
    if ed.deprecated {
        let _ = writeln!(src, "///");
        let _ = writeln!(src, "/// # Deprecated");
        let _ = writeln!(src, "#[deprecated]");
    }
    let _ = writeln!(src, "#[repr(transparent)]");
    let _ = writeln!(
        src,
        "#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]"
    );
    let _ = writeln!(src, "pub struct {name}(pub i32);");
    let _ = writeln!(
        src,
        "    #[allow(non_upper_case_globals, reason = \"protobuf enum names\")]"
    );
    let _ = writeln!(src, "impl {name} {{");
    for v in &values {
        if let Some(c) = ed.value_comments(v.number) {
            emit_doc_comments(src, c, "    ");
            let _ = writeln!(src, "    ///");
        }
        let _ = writeln!(src, "    /// Enum value `{}` ({}).", v.name, v.number);
        if ed.is_value_deprecated(v.number) {
            let _ = writeln!(src, "    ///");
            let _ = writeln!(src, "    /// # Deprecated");
            let _ = writeln!(src, "    #[deprecated]");
        }
        let _ = writeln!(
            src,
            "    pub const {}: {name} = {name}({});",
            v.name, v.number
        );
        for a in &v.aliases {
            let _ = writeln!(src, "    pub const {a}: {name} = {name}({});", v.number);
        }
    }
    let _ = writeln!(src, "    fn constant_name(self) -> Option<&'static str> {{");
    let _ = writeln!(
        src,
        "        #[allow(unreachable_patterns, reason = \"open enums\")]"
    );
    let _ = writeln!(src, "        Some(match self.0 {{");
    for v in &values {
        let _ = writeln!(src, "            {} => \"{}\",", v.number, v.name);
    }
    let _ = writeln!(src, "            _ => return None,");
    let _ = writeln!(src, "        }})");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl From<{name}> for i32 {{");
    let _ = writeln!(src, "    fn from(v: {name}) -> i32 {{ v.0 }}");
    let _ = writeln!(src, "}}");
    if ed.closed {
        let _ = writeln!(src, "impl TryFrom<i32> for {name} {{");
        let _ = writeln!(src, "    type Error = UnknownEnumValue<Self>;");
        let _ = writeln!(
            src,
            "    fn try_from(val: i32) -> Result<Self, Self::Error> {{"
        );
        let _ = writeln!(
            src,
            "        if <Self as Enum>::is_known(val) {{ Ok(Self(val)) }} else {{ Err(UnknownEnumValue::new(pbrs::__internal::Private, val)) }}"
        );
        let _ = writeln!(src, "    }}");
        let _ = writeln!(src, "}}");
    } else {
        let _ = writeln!(src, "impl From<i32> for {name} {{");
        let _ = writeln!(src, "    fn from(val: i32) -> Self {{ Self(val) }}");
        let _ = writeln!(src, "}}");
    }
    let _ = writeln!(src, "impl Default for {name} {{");
    let _ = writeln!(src, "    fn default() -> Self {{ Self({first}) }}");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl std::fmt::Debug for {name} {{");
    let _ = writeln!(
        src,
        "    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{"
    );
    let _ = writeln!(
        src,
        "        if let Some(n) = self.constant_name() {{ write!(f, \"{name}::{{n}}\") }} else {{ write!(f, \"{name}::from({{}})\", self.0) }}"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl pbrs::__internal::SealedInternal for {name} {{}}");
    let _ = writeln!(
        src,
        "impl pbrs::Proxied for {name} {{ type View<'msg> = {name}; }}"
    );
    let _ = writeln!(src, "impl pbrs::AsView for {name} {{");
    let _ = writeln!(src, "    type Proxied = Self;");
    let _ = writeln!(src, "    fn as_view(&self) -> Self {{ *self }}");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl<'msg> pbrs::IntoView<'msg> for {name} {{");
    let _ = writeln!(
        src,
        "    fn into_view<'shorter>(self) -> Self where 'msg: 'shorter {{ self }}"
    );
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl Enum for {name} {{");
    let _ = writeln!(src, "    const NAME: &'static str = \"{name}\";");
    if known.is_empty() {
        let _ = writeln!(src, "    fn is_known(_: i32) -> bool {{ false }}");
    } else {
        let _ = writeln!(
            src,
            "    fn is_known(value: i32) -> bool {{ matches!(value, {known}) }}"
        );
    }
    let _ = writeln!(src, "}}");
}

pub(crate) fn emit_nested_mods(src: &mut String, messages: &[String], enums: &[String]) {
    let msg_set: std::collections::BTreeSet<&str> = messages.iter().map(|s| s.as_str()).collect();
    #[derive(Default)]
    struct Node {
        children: std::collections::BTreeMap<String, Node>,
        items: Vec<(String, String)>,
    }
    let mut root = Node::default();
    let mut add = |full: &str, is_msg: bool| {
        let ident = rust_ident(full);
        let last = ident_last(full);
        let mut cur = full;
        let mut path = Vec::new();
        while let Some((parent, _)) = cur.rsplit_once('.') {
            if !msg_set.contains(parent) {
                break;
            }
            path.push(to_snake(&ident_last(parent)));
            cur = parent;
        }
        if path.is_empty() {
            return;
        }
        path.reverse();
        let mut node = &mut root;
        for p in &path {
            node = node.children.entry(p.clone()).or_default();
        }
        node.items.push((last.clone(), ident.clone()));
        if is_msg {
            node.items
                .push((format!("{last}View"), format!("{ident}View")));
            node.items
                .push((format!("{last}Mut"), format!("{ident}Mut")));
        }
    };
    for m in messages {
        add(m, true);
    }
    for e in enums {
        add(e, false);
    }
    fn emit_node(src: &mut String, name: &str, node: &Node, depth: usize) {
        let _ = writeln!(src, "pub mod {name} {{");
        let prefix = "super::".repeat(depth);
        let mut sorted_items = node.items.clone();
        sorted_items.sort();
        for (export, ident) in &sorted_items {
            if export == ident {
                let _ = writeln!(src, "    pub use {prefix}{ident};");
            } else {
                let _ = writeln!(src, "    pub use {prefix}{ident} as {export};");
            }
        }
        for (child_name, child) in &node.children {
            emit_node(src, child_name, child, depth + 1);
        }
        let _ = writeln!(src, "}}");
    }
    for (name, node) in &root.children {
        emit_node(src, name, node, 1);
    }
}
