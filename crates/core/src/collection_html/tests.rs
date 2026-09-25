use super::*;

fn base() -> Url {
    Url::parse("https://collection.invalid/base/").unwrap()
}
// Frozen v1-v3 extraction algorithm is a test oracle, never an unbounded
// production fallback. These small fixed specimens exercise exact output order.
fn legacy(raw: &str) -> (String, Vec<Url>) {
    let html = Html::parse_document(raw);
    let text = html
        .root_element()
        .descendants()
        .filter_map(|node| {
            let value = node.value().as_text()?;
            (!node.ancestors().any(|parent| hidden(parent.value()))).then_some(value.text.as_ref())
        })
        .collect::<Vec<&str>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let selector = Selector::parse("a[href]").unwrap();
    let links = html
        .select(&selector)
        .take(1000)
        .filter_map(|anchor| base().join(anchor.value().attr("href")?).ok())
        .filter_map(|url| policy::validate_https_url(url.as_str()).ok())
        .collect();
    (text, links)
}
#[test]
fn ordinary_and_malformed_markup_keeps_exact_legacy_text_and_anchor_order() {
    let cases = [
        "", "<p>A <b>B</b>C</p> D", "A\u{a0}B\n\tC &amp; &#x1f642;",
        "<script>secret()</script><style>x{}</style><svg><text>secret</text></svg><p>visible</p><noscript>hidden</noscript>",
        "<template><p>hidden <a href='/template'>link</a></p></template><a href='/shown'>shown</a>",
        "<table>before<tr><td>cell<a href='/cell'>x</table>after<a href='/last'>last",
        "<b><i>format</b>adoption</i><a href='/first'>1<a href='/second'>2",
        "<svg><a href='/svg'>hidden link</a></svg><a href='http://bad.invalid'>bad</a>",
        "<!--comment--><!doctype html><html><head><title>Title</title></head><body><a href='?x=1&amp;y=2'>link</a></body>",
        "<textarea>&lt;b&gt;text</textarea><xmp><p>raw</xmp><plaintext>literal<b>suffix",
        "\u{feff}<p>null\0replacement\r\nnext</p>",
    ];
    for raw in cases {
        assert_eq!(extract(raw, &base()).unwrap(), legacy(raw), "{raw}");
    }
    // Chunk boundaries in entities, script markers, UTF-8 and attributes must
    // not change the parser's text merging or allocation-order anchor output.
    for padding in 490..=520 {
        let raw = format!(
            "{}&amp;<script>\"</script>\"</script><p>é🙂</p><a href='/x'>x</a>",
            " ".repeat(padding)
        );
        assert_eq!(extract(&raw, &base()).unwrap(), legacy(&raw));
    }
}
#[test]
fn first_thousand_anchor_candidates_precede_filtering_and_keep_duplicates() {
    let raw = format!(
        "{}{}<a href='/excluded'>late</a>",
        "<a href='http://bad.invalid'>bad</a>".repeat(999),
        "<svg><a href='/hidden'>hidden</a></svg>"
    );
    let result = extract(&raw, &base()).unwrap();
    assert_eq!(result, legacy(&raw));
    assert_eq!(result.1, vec![base().join("/hidden").unwrap()]);
    let raw = "<a href='/same'>a</a><a href='/same'>b</a>";
    assert_eq!(extract(raw, &base()).unwrap().1.len(), 2);
}
#[test]
fn deep_wide_and_adversarial_sources_refuse_without_partial_output() {
    let deep = format!("{}visible{}", "<div>x".repeat(129), "</div>".repeat(129));
    assert_eq!(extract(&deep, &base()), Err(HtmlLimit::Depth));
    let wide = "<p>x</p>".repeat(10_000);
    assert!(matches!(
        extract(&wide, &base()),
        Err(HtmlLimit::AdmissionUnits | HtmlLimit::Nodes)
    ));
    let formatting = format!("{}<p>repair", "<b><i><u>".repeat(2000));
    assert!(extract(&formatting, &base()).is_err());
    let long_comment = format!("before<!--{}-->after", "x".repeat(20 * 1024));
    assert_eq!(
        extract(&long_comment, &base()),
        Err(HtmlLimit::PendingToken)
    );
    let long_tag = format!("<a href='{}'>text</a>", "x".repeat(20 * 1024));
    assert_eq!(extract(&long_tag, &base()), Err(HtmlLimit::PendingToken));
    let attributes = (0..129).map(|i| format!(" a{i}='x'")).collect::<String>();
    assert_eq!(
        extract(&format!("<p{attributes}>prefix</p>"), &base()),
        Err(HtmlLimit::Attributes)
    );
    // Repeated tokenizer errors cannot reset the unfinished-token byte window.
    let duplicate_attributes = format!("<p{}>", " a='x'".repeat(4000));
    assert!(matches!(
        extract(&duplicate_attributes, &base()),
        Err(HtmlLimit::PendingToken | HtmlLimit::Tokens)
    ));
}
#[test]
fn input_text_and_link_byte_caps_have_exact_accept_refuse_boundaries() {
    assert_eq!(
        extract(&"x".repeat(PAGE_BYTES + 1), &base()),
        Err(HtmlLimit::Input)
    );
    assert_eq!(
        extract(&"x".repeat(LIMITS.text_bytes), &base())
            .unwrap()
            .0
            .len(),
        LIMITS.text_bytes
    );
    assert_eq!(
        extract(&"x".repeat(LIMITS.text_bytes + 1), &base()),
        Err(HtmlLimit::Text)
    );
    let hidden = format!("<script>{}</script><p>visible</p>", "x".repeat(600 * 1024));
    assert_eq!(extract(&hidden, &base()).unwrap().0, "visible");
    let tree = parse("<a href='/a'>one</a><a href='/b'>two</a>", LIMITS).unwrap();
    let bytes =
        base().join("/a").unwrap().as_str().len() + base().join("/b").unwrap().as_str().len();
    assert!(extract_tree(
        &tree,
        &base(),
        Limits {
            link_bytes: bytes,
            ..LIMITS
        }
    )
    .is_ok());
    assert_eq!(
        extract_tree(
            &tree,
            &base(),
            Limits {
                link_bytes: bytes - 1,
                ..LIMITS
            }
        ),
        Err(HtmlLimit::Links)
    );
}
#[test]
fn token_and_weighted_admission_refusal_stops_forwarding_even_at_eof() {
    for reason in [
        HtmlLimit::Tokens,
        HtmlLimit::AdmissionUnits,
        HtmlLimit::Nodes,
    ] {
        let limits = match reason {
            HtmlLimit::Tokens => Limits {
                tokens: 0,
                ..LIMITS
            },
            HtmlLimit::AdmissionUnits => Limits {
                admission_units: 0,
                ..LIMITS
            },
            HtmlLimit::Nodes => Limits { nodes: 1, ..LIMITS },
            _ => unreachable!(),
        };
        let builder = BoundedBuilder {
            builder: TreeBuilder::new(HtmlTreeSink::new(Html::new_document()), Default::default()),
            limits,
            counters: Counters::default(),
        };
        let _ = builder.process_token(Token::CharacterTokens(StrTendril::from_slice("prefix")), 1);
        assert_eq!(builder.counters.refusal.get(), Some(reason));
        let nodes = builder.nodes();
        for _ in 0..10 {
            let _ =
                builder.process_token(Token::CharacterTokens(StrTendril::from_slice("ignored")), 1);
        }
        let _ = builder.process_token(Token::EOFToken, 1);
        builder.end();
        assert_eq!(builder.nodes(), nodes);
        assert_eq!(builder.counters.tokens.get(), 1);
    }
}

#[test]
fn interior_bom_at_feed_boundary_and_script_resume_match_legacy_driver() {
    for raw in [
        format!("{}\u{feff}tail", "x".repeat(CHUNK_BYTES)),
        format!("{}\u{feff}tail", " ".repeat(CHUNK_BYTES)),
        format!("{}<script>x</script>\u{feff}tail", "x".repeat(493)),
        "\u{feff}initial<script>x</script>\u{feff}after".into(),
        "\u{feff}\u{feff}second".into(),
    ] {
        assert_eq!(extract(&raw, &base()).unwrap(), legacy(&raw));
    }
}
