use relm4::gtk::{self, glib, prelude::*, subclass::prelude::*};

#[derive(Default)]
pub struct Layout;

#[glib::object_subclass]
impl ObjectSubclass for Layout {
    const NAME: &'static str = "GatefoldSquareLayout";
    type Type = SquareLayout;
    type ParentType = gtk::LayoutManager;
}

impl ObjectImpl for Layout {}

impl LayoutManagerImpl for Layout {
    fn request_mode(&self, _: &gtk::Widget) -> gtk::SizeRequestMode {
        gtk::SizeRequestMode::HeightForWidth
    }

    fn measure(
        &self,
        widget: &gtk::Widget,
        orientation: gtk::Orientation,
        for_size: i32,
    ) -> (i32, i32, i32, i32) {
        let child = widget.first_child();
        if orientation == gtk::Orientation::Horizontal {
            let (min, nat) = child
                .map(|child| {
                    let (min, nat, _, _) = child.measure(orientation, -1);
                    (min, nat)
                })
                .unwrap_or((0, 0));
            (min, nat, -1, -1)
        } else {
            let size = if for_size >= 0 { for_size } else { 0 };
            (size, size, -1, -1)
        }
    }

    fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
        if let Some(child) = widget.first_child() {
            child.allocate(width, height, baseline, None);
        }
    }
}

glib::wrapper! {
    pub struct SquareLayout(ObjectSubclass<Layout>) @extends gtk::LayoutManager;
}

impl SquareLayout {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
