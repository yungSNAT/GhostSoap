use super::*;

#[test]
fn keeps_main_text_only() {
    let html = "<html><head><title>T</title></head><body><nav>Menu</nav>\
        <main class=\"x\"><h1>Privacy</h1><p>Email <b>privacy@x.au</b></p></main>\
        <footer>2026</footer></body></html>";
    assert_eq!(main_text(html), "Privacy Email privacy@x.au");
}

#[test]
fn lists_the_fields_a_page_serves() {
    let html = "<form>1 < 2 <INPUT type=text NAME=firstName><input required name='email'>\
        <input type=\"checkbox\" data-name=\"x\" name=\"item&#91;1&#93;\"/>\
        <textarea\nname = \"message\"></textarea><select name=\"state\"></select>\
        <input name=\"email\"><input type=\"submit\"><button name=\"go\">Go</button>\
        <label for=\"name\">Name</label></form>";
    assert_eq!(
        field_names(html),
        ["firstName", "email", "item[1]", "message", "state"]
    );
}

#[test]
fn skips_controls_a_user_cannot_fill_in() {
    let html = "<input type=hidden name=email><input TYPE=\"Hidden\" name=a>\
        <input type=submit name=go><input type=reset name=r><input type=button name=b>\
        <input type=image name=i><input name=off disabled><select name=s disabled></select>\
        <textarea name=ro readonly></textarea><input type=email name=kept>";
    assert_eq!(field_names(html), ["kept"]);
}

#[test]
fn controls_hidden_by_the_hidden_attribute_are_skipped() {
    let html = "<?pi hidden ?><input type=\"text\" name=\"email\" hidden><div hidden><p>\
        <input name=inner></p><div><select name=deep></select></div></div>\
        <section hidden/><input name=under></section>\
        <img hidden src=x><br hidden><input name=seen>";
    assert_eq!(field_names(html), ["seen"]);
}

#[test]
fn controls_in_a_disabled_fieldset_cannot_be_filled_in() {
    let html = "<fieldset disabled><input name=off><fieldset><select name=deep></select>\
        </fieldset></fieldset><fieldset><input name=on></fieldset><input name=after>";
    assert_eq!(field_names(html), ["on", "after"]);
}

#[test]
fn readonly_applies_only_to_text_controls() {
    let html = "<select name=s readonly></select><input type=checkbox name=c readonly>\
        <input type=radio name=r readonly><input name=t readonly><input type=email name=e readonly>";
    assert_eq!(field_names(html), ["s", "c", "r"]);
}

#[test]
fn the_contents_of_a_textarea_are_text() {
    let html = "<textarea name=\"message\"><input name=\"email\"></TEXTAREA><input name=after>";
    assert_eq!(field_names(html), ["message", "after"]);
}

#[test]
fn ignores_fields_in_comments_scripts_and_templates() {
    let html = "<!-- <input name=\"old\"> --><script>'<input name=\"js\">'</script>\
        <template><input name=\"later\"></template><noscript><input name=\"ns\"></noscript>\
        <input data-x=\"<input name=decoy>\" name=\"real\">";
    assert_eq!(field_names(html), ["real"]);
}

#[test]
fn a_page_without_fields_lists_none() {
    assert!(field_names("<p>Use our form at <a href=\"/f\">this page</a>.</p>").is_empty());
    assert!(field_names("<input type=\"text\"><input name=\"\">").is_empty());
}

#[test]
fn a_tag_can_have_an_unquoted_attribute() {
    assert_eq!(
        main_text("<main id=policy>Policy text</main>"),
        "Policy text"
    );
}

#[test]
fn falls_back_to_body_and_skips_scripts_and_comments() {
    let html = "<BODY><script>var a = '<p>';</script><!-- note -->Hello\n\n  <STYLE>p{}</STYLE>world</BODY>";
    assert_eq!(main_text(html), "Hello world");
}

#[test]
fn does_not_take_a_tag_that_only_starts_with_main() {
    let html = "<body><mainmenu>Menu</mainmenu><main-nav>Nav</main-nav><p>Text</p></body>";
    assert_eq!(main_text(html), "Menu Nav Text");
}

#[test]
fn ignores_main_tags_in_comments_scripts_and_templates() {
    let html = "<!-- <main>Old placeholder</main> --><html><body>\
        <template><main>Template</main></template>\
        <script>document.write('<main>Fake</main>')</script>\
        <main>Current <script>'</main>'</script>policy</main></body></html>";
    assert_eq!(main_text(html), "Current policy");
}

#[test]
fn skipped_elements_end_at_their_own_end_tag() {
    let html = "<body><script>const e = \"</scripture><main>Decoy</main>\";</script>\
        <template><template>x</template><main>Decoy</main></template>\
        <template><script>\"</template>\"</script><main>Decoy</main></template>\
        <svg><svg/><svg><text>t</text></svg><main>Decoy</main></svg>\
        <main>Actual policy</main></body>";
    assert_eq!(main_text(html), "Actual policy");
}

#[test]
fn cdata_and_quoted_end_tags_do_not_end_skipped_elements() {
    for skipped in [
        "<svg><![CDATA[x > </svg><main>Decoy</main>]]></svg>",
        "<script>x</script title=\"><main>Decoy</main>\">",
    ] {
        let html = format!("<body>{skipped}<main>Actual</main></body>");
        assert_eq!(main_text(&html), "Actual");
    }
    for html in [
        "<body><script>x</script >Text",
        "<body>Text<script>x</script",
    ] {
        assert_eq!(main_text(html), "Text");
    }
}

#[test]
fn unclosed_skipped_elements_hide_their_remaining_text() {
    for tail in [
        "<template><!-- </template> --> unclosed < 3 <main>Hidden</main>",
        "<script>unclosed <main>Hidden</main>",
    ] {
        assert_eq!(main_text(&format!("<body>Visible {tail}")), "Visible");
    }
}

#[test]
fn a_self_closing_svg_skips_only_itself() {
    for svg in ["<svg/>", "<svg aria-label=\"a > b\"/>"] {
        let html = format!("<body>{svg}<main>Actual policy</main></body>");
        assert_eq!(main_text(&html), "Actual policy");
    }
}

#[test]
fn a_less_than_sign_that_starts_no_tag_is_text() {
    let html = "<main>Threshold: 5 < 10 > 0. Requests remain available.</main>";
    assert_eq!(
        main_text(html),
        "Threshold: 5 < 10 > 0. Requests remain available."
    );
    assert_eq!(main_text("<main>a <3 b</main><p>Not main</p>"), "a <3 b");
}

#[test]
fn nested_main_keeps_the_whole_first_element() {
    let html = "<main>Before <main>Inner</main> After</main><main>Second</main>";
    assert_eq!(main_text(html), "Before Inner After");
}

#[test]
fn a_quoted_angle_bracket_does_not_end_a_tag() {
    let html = "<main><a title=\"a > b\" data-x = 'c>d'>Link</a> text</main>";
    assert_eq!(main_text(html), "Link text");
}

#[test]
fn decodes_entities() {
    assert_eq!(
        main_text("A&amp;B &lt;x&gt; &#39;q&#x27; a&nbsp;b &bogus; & c"),
        "A&B <x> 'q' a b &bogus; & c"
    );
    // The `;` must be at most 10 bytes after the `&`.
    assert_eq!(main_text("&#00000065; &#000000065;"), "A &#000000065;");
}

#[test]
fn many_unterminated_ampersands_decode_in_linear_time() {
    // Quadratic scanning would take minutes on this input.
    let text = "&".repeat(500_000);
    assert_eq!(decode_entities(&text), text);
}

#[test]
fn detects_html_by_media_type_or_markup() {
    let page = b"\xEF\xBB\xBF \n<!DOCTYPE HTML><html></html>";
    assert!(is_html(Some("text/html; charset=utf-8"), b""));
    assert!(is_html(Some("TEXT/HTML"), b""));
    assert!(is_html(None, page));
    assert!(is_html(None, b"<HTML lang=\"en\">"));
    assert!(!is_html(Some("application/pdf"), page));
    assert!(!is_html(None, b"%PDF-1.7"));
}

#[test]
fn decodes_cloudflare_protected_emails() {
    let html = "<a href=\"/cdn-cgi/l/email-protection\" class=\"__cf_email__\" \
        data-cfemail=\"4232302b3423213b02273a232f322e276c212d2f6c2337\">[email protected]</a> \
        <a href=\"/cdn-cgi/l/email-protection#5a323f362a1a3f223b372a363f74393537743b2f\">Help</a> \
        <span data-cfemail=\"zz\"></span><span data-cfemail=\"4\"></span>\
        <span data-cfemail=\"4232302b3423213b02273a232f322e276c212d2f6c2337z\"></span>\
        <span data-cfemail=\"4232302b3423213b02273a232f322e276c212d2f6c2337'x\"></span>\
        <a href=\"/cdn-cgi/l/email-protection#4232302b3423213b02273a232f322e276c212d2f6c2337'x\">";
    assert_eq!(
        cloudflare_emails(html),
        ["privacy@example.com.au", "help@example.com.au"]
    );
    assert!(cloudflare_emails("<p>privacy@example.com.au</p>").is_empty());
}
