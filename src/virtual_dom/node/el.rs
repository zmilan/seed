use super::super::{At, AtValue, Attrs, CSSValue, Listener, Node, St, Style, Tag, Text};
use crate::app::MessageMapper;
use crate::browser::{
    dom::{virtual_dom_bridge, LifecycleHooks, Namespace},
    util,
};
use std::borrow::Cow;

/// A component in our virtual DOM.
/// [MDN reference](https://developer.mozilla.org/en-US/docs/Web/API/Element)
/// [`web_sys` reference](https://rustwasm.github.io/wasm-bindgen/api/web_sys/struct.Element.html)
#[derive(Debug)] // todo: Custom debug implementation where children are on new lines and indented.
pub struct El<Ms: 'static> {
    // Ms is a message type, as in part of TEA.
    // We call this 'El' instead of 'Element' for brevity, and to prevent
    // confusion with web_sys::Element.
    pub tag: Tag,
    pub attrs: Attrs,
    pub style: Style,
    pub listeners: Vec<Listener<Ms>>,
    pub children: Vec<Node<Ms>>,
    /// The actual web element/node
    pub node_ws: Option<web_sys::Node>,
    pub namespace: Option<Namespace>,
    pub hooks: LifecycleHooks<Ms>,
}

impl<Ms: 'static, OtherMs: 'static> MessageMapper<Ms, OtherMs> for El<Ms> {
    type SelfWithOtherMs = El<OtherMs>;
    /// Maps an element's message to have another message.
    ///
    /// This allows third party components to integrate with your application without
    /// having to know about your Msg type beforehand.
    ///
    /// # Note
    /// There is an overhead to calling this versus keeping all messages under one type.
    /// The deeper the nested structure of children, the more time this will take to run.
    fn map_msg(self, f: impl FnOnce(Ms) -> OtherMs + 'static + Clone) -> El<OtherMs> {
        El {
            tag: self.tag,
            attrs: self.attrs,
            style: self.style,
            listeners: self
                .listeners
                .into_iter()
                .map(|l| l.map_msg(f.clone()))
                .collect(),
            children: self
                .children
                .into_iter()
                .map(|c| c.map_msg(f.clone()))
                .collect(),
            node_ws: self.node_ws,
            namespace: self.namespace,
            hooks: self.hooks.map_msg(f),
        }
    }
}

impl<Ms: 'static, OtherMs: 'static> MessageMapper<Ms, OtherMs> for Vec<El<Ms>> {
    type SelfWithOtherMs = Vec<El<OtherMs>>;
    fn map_msg(self, f: impl FnOnce(Ms) -> OtherMs + 'static + Clone) -> Vec<El<OtherMs>> {
        self.into_iter().map(|el| el.map_msg(f.clone())).collect()
    }
}

impl<Ms> El<Ms> {
    /// Create an empty element, specifying only the tag
    pub fn empty(tag: Tag) -> Self {
        Self {
            tag,
            attrs: Attrs::empty(),
            style: Style::empty(),
            listeners: Vec::new(),
            children: Vec::new(),
            node_ws: None,
            namespace: None,
            hooks: LifecycleHooks::new(),
        }
    }

    /// Create an empty SVG element, specifying only the tag
    pub fn empty_svg(tag: Tag) -> Self {
        let mut el = El::empty(tag);
        el.namespace = Some(Namespace::Svg);
        el
    }

    // todo: Return El instead of Node here? (Same with from_html)
    /// Create elements from a markdown string.
    pub fn from_markdown(markdown: &str) -> Vec<Node<Ms>> {
        let parser = pulldown_cmark::Parser::new(markdown);
        let mut html_text = String::new();
        pulldown_cmark::html::push_html(&mut html_text, parser);

        Self::from_html(&html_text)
    }

    /// Create elements from an HTML string.
    pub fn from_html(html: &str) -> Vec<Node<Ms>> {
        // Create a web_sys::Element, with our HTML wrapped in a (arbitrary) span tag.
        // We allow web_sys to parse into a DOM tree, then analyze the tree to create our vdom
        // element.
        let wrapper = util::document()
            .create_element("placeholder")
            .expect("Problem creating web-sys element");
        wrapper.set_inner_html(html);

        let mut result = Vec::new();
        let children = wrapper.child_nodes();
        for i in 0..children.length() {
            let child = children
                .get(i)
                .expect("Can't find child in raw html element.");

            if let Some(child_vdom) = virtual_dom_bridge::node_from_ws(&child) {
                result.push(child_vdom)
            }
        }
        result
    }

    /// Add a new child to the element
    pub fn add_child(&mut self, element: Node<Ms>) -> &mut Self {
        self.children.push(element);
        self
    }

    /// Add an attribute (eg class, or href)
    pub fn add_attr(
        &mut self,
        key: impl Into<Cow<'static, str>>,
        val: impl Into<AtValue>,
    ) -> &mut Self {
        self.attrs
            .vals
            .insert(key.into().as_ref().into(), val.into());
        self
    }

    /// Add a class. May be cleaner than `add_attr`
    pub fn add_class(&mut self, name: impl Into<Cow<'static, str>>) -> &mut Self {
        let name = name.into();
        self.attrs
            .vals
            .entry(At::Class)
            .and_modify(|at_value| match at_value {
                AtValue::Some(v) => {
                    if !v.is_empty() {
                        *v += " ";
                    }
                    *v += name.as_ref();
                }
                _ => *at_value = AtValue::Some(name.clone().into_owned()),
            })
            .or_insert(AtValue::Some(name.into_owned()));
        self
    }

    /// Add a new style (eg display, or height)
    pub fn add_style(&mut self, key: impl Into<St>, val: impl Into<CSSValue>) -> &mut Self {
        self.style.vals.insert(key.into(), val.into());
        self
    }

    /// Add a new listener
    pub fn add_listener(&mut self, listener: Listener<Ms>) -> &mut Self {
        self.listeners.push(listener);
        self
    }

    /// Add a text node to the element. (ie between the HTML tags).
    pub fn add_text(&mut self, text: impl Into<Cow<'static, str>>) -> &mut Self {
        self.children.push(Node::Text(Text::new(text)));
        self
    }

    /// Replace the element's text.
    /// Removes all text nodes from element, then adds the new one.
    pub fn replace_text(&mut self, text: impl Into<Cow<'static, str>>) -> &mut Self {
        self.children.retain(|node| !node.is_text());
        self.children.push(Node::new_text(text));
        self
    }

    // Pull text from child text nodes
    pub fn get_text(&self) -> String {
        self.children
            .iter()
            .filter_map(|child| match child {
                Node::Text(text_node) => Some(text_node.text.to_string()),
                _ => None,
            })
            .collect()
    }

    /// Remove websys nodes.
    pub fn strip_ws_nodes_from_self_and_children(&mut self) {
        self.node_ws.take();
        for child in &mut self.children {
            child.strip_ws_nodes_from_self_and_children();
        }
    }

    /// Is it a custom element?
    pub fn is_custom(&self) -> bool {
        // @TODO: replace with `matches!` macro once stable
        if let Tag::Custom(_) = self.tag {
            true
        } else {
            false
        }
    }
}

/// Allow the user to clone their Els. Note that there's no easy way to clone the
/// closures within listeners or lifestyle hooks, so we omit them.
impl<Ms: Clone> Clone for El<Ms> {
    fn clone(&self) -> Self {
        Self {
            tag: self.tag.clone(),
            attrs: self.attrs.clone(),
            style: self.style.clone(),
            children: self.children.clone(),
            node_ws: self.node_ws.clone(),
            listeners: self.listeners.clone(),
            namespace: self.namespace.clone(),
            hooks: LifecycleHooks::new(),
        }
    }
}

impl<Ms> PartialEq for El<Ms> {
    fn eq(&self, other: &Self) -> bool {
        // todo Again, note that the listeners check only checks triggers.
        // Don't check children.
        self.tag == other.tag
            && self.attrs == other.attrs
            && self.style == other.style
            && self.listeners == other.listeners
            && self.namespace == other.namespace
    }
}
