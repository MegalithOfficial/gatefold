use std::cell::{Cell, RefCell};

use relm4::gtk::{self, glib, graphene, gsk, prelude::*, subclass::prelude::*};

#[derive(Default)]
pub struct Inner {
    lifted: RefCell<Option<gtk::Widget>>,
    lift: Cell<f32>,
}

#[glib::object_subclass]
impl ObjectSubclass for Inner {
    const NAME: &'static str = "GatefoldLane";
    type Type = Lane;
    type ParentType = gtk::Widget;
}

impl ObjectImpl for Inner {
    fn dispose(&self) {
        while let Some(child) = self.obj().first_child() {
            child.unparent();
        }
    }
}

impl WidgetImpl for Inner {
    fn request_mode(&self) -> gtk::SizeRequestMode {
        gtk::SizeRequestMode::HeightForWidth
    }

    fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
        let (mut min, mut nat) = (0, 0);
        let mut child = self.obj().first_child();
        while let Some(widget) = child {
            if widget.should_layout() {
                let (child_min, child_nat, _, _) = widget.measure(orientation, for_size);
                if orientation == gtk::Orientation::Horizontal {
                    min = min.max(child_min);
                    nat = nat.max(child_nat);
                } else {
                    min += child_min;
                    nat += child_nat;
                }
            }
            child = widget.next_sibling();
        }

        (min, nat, -1, -1)
    }

    fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
        let lifted = self.lifted.borrow().clone();
        let mut y = 0;
        let mut child = self.obj().first_child();
        while let Some(widget) = child {
            if widget.should_layout() {
                let (_, height, _, _) = widget.measure(gtk::Orientation::Vertical, width);
                let top = if lifted.as_ref() == Some(&widget) {
                    self.lift.get()
                } else {
                    y as f32
                };
                let transform = gsk::Transform::new().translate(&graphene::Point::new(0.0, top));
                widget.allocate(width, height, -1, Some(transform));
                y += height;
            }
            child = widget.next_sibling();
        }
    }

    fn snapshot(&self, snapshot: &gtk::Snapshot) {
        let widget = self.obj();
        let lifted = self.lifted.borrow().clone();
        let mut child = widget.first_child();
        while let Some(current) = child {
            if lifted.as_ref() != Some(&current) {
                widget.snapshot_child(&current, snapshot);
            }
            child = current.next_sibling();
        }
        if let Some(lifted) = lifted {
            widget.snapshot_child(&lifted, snapshot);
        }
    }
}

glib::wrapper! {
    pub struct Lane(ObjectSubclass<Inner>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Lane {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl Lane {
    pub fn append(&self, child: &impl IsA<gtk::Widget>) {
        child.set_parent(self);
    }

    pub fn remove(&self, child: &impl IsA<gtk::Widget>) {
        if self.imp().lifted.borrow().as_ref() == Some(child.upcast_ref()) {
            self.lift(None, 0.0);
        }
        child.unparent();
    }

    pub fn insert_after(&self, child: &impl IsA<gtk::Widget>, sibling: Option<&gtk::Widget>) {
        child.insert_after(self, sibling);
        self.queue_resize();
    }

    pub fn lift(&self, child: Option<&gtk::Widget>, top: f32) {
        let inner = self.imp();
        *inner.lifted.borrow_mut() = child.cloned();
        inner.lift.set(top);
        self.queue_allocate();
    }
}
