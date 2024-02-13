//! `'static` versions of the events - the form in which they're stored when awaiting a pump,
//! and are converted on-the-fly to the more ergonomic event types.

// **BONK BONK BONK BONK* NO PREMATURE OPTIMIZATION *BONK BONK BONK BONK BON-*

// use crate::tool::AvailableAxes;
// /// Weakly refers to a continuous block of stylus data on some external deque.
// /// Fields are popped from that external deque `[X,Y,<packed_axes from LSB to MSB>]`
// ///
// /// This is done to lower the size overhead of the events, where a lot of the events are
// /// only a few bytes a whole `Pose` is many times larger.
// #[derive(Copy, Clone, Debug)]
// pub struct WeakPose {
//     packed_axes: AvailableAxes,
// }
// impl WeakPose {
//     /// Query how many floats are to be pulled from the external deque.
//     pub fn len(self) -> usize {
//         //X,Y
//         2
//         //Every other axis is at least one
//         + self.packed_axes.bits().count_ones() as usize
//         //Some axes have an extra (to add more this should also become a popcnt)
//         + usize::from(self.packed_axes.intersects(AvailableAxes::TILT))
//     }
//     /// Given a data source, pop enough data to fulfill this weak pose, creating a full
//     /// pose. None if not enough data to construct the expected pose.
//     ///
//     /// On success, the `cursor` is automagically advanced forward to the next pose in the slice.
//     /// # Safety
//     /// All values in `cursor` must be non-NaN.
//     pub unsafe fn extract(self, cursor: &mut &[f32]) -> Option<super::Pose> {
//         let len = self.len();
//         let slice = (*cursor).get(..len)?;
//         let mut iter = slice.iter().copied();
//         // Safety: Non-Nan. Deferred to this fn's contract.
//         let pose = unsafe {
//             super::Pose::new(
//                 slice[..2].try_into().unwrap(),
//                 self.packed_axes
//                     .contains(AvailableAxes::DISTANCE)
//                     .then(|| iter.next().unwrap()),
//                 self.packed_axes
//                     .contains(AvailableAxes::PRESSURE)
//                     .then(|| iter.next().unwrap()),
//                 self.packed_axes
//                     .contains(AvailableAxes::TILT)
//                     .then(|| [iter.next().unwrap(), iter.next().unwrap()]),
//                 self.packed_axes
//                     .contains(AvailableAxes::ROLL)
//                     .then(|| iter.next().unwrap()),
//                 todo!(),
//                 self.packed_axes
//                     .contains(AvailableAxes::SLIDER)
//                     .then(|| iter.next().unwrap()),
//             )
//         };
//         pose.debug_assert_not_nan();
//         //advance cursor.
//         // infallible, since `get` above succeeded.
//         *cursor = &(*cursor)[len..];
//     }
// }

#[derive(Clone, Debug)]
pub enum ToolEvent<Id> {
    Added,
    Removed,
    In { tablet: Id },
    Down,
    Button(()),
    Pose(super::Pose),
    Frame(Option<super::FrameTimestamp>),
    Up,
    Out,
}
impl<Id> ToolEvent<Id> {
    // Can't impl `From`, due to conflict with `From<T> for T` :(
    pub fn id_into<Into: From<Id>>(self) -> ToolEvent<Into> {
        match self {
            Self::Added => ToolEvent::Added,
            Self::Removed => ToolEvent::Removed,
            Self::In { tablet } => ToolEvent::In {
                tablet: Into::from(tablet),
            },
            Self::Down => ToolEvent::Down,
            Self::Button(v) => ToolEvent::Button(v),
            Self::Pose(v) => ToolEvent::Pose(v),
            Self::Frame(v) => ToolEvent::Frame(v),
            Self::Up => ToolEvent::Up,
            Self::Out => ToolEvent::Out,
        }
    }
}
/// Events associated with a specific `Tablet`.
#[derive(Clone, Debug)]
pub enum TabletEvent {
    Added,
    Removed,
}
/// Events associated with a specific `Pad`.
#[derive(Clone, Debug)]
pub enum PadEvent<Id> {
    Added,
    Removed,
    Group { group: Id, event: PadGroupEvent },
}
impl<Id> PadEvent<Id> {
    // Can't impl `From`, due to conflict with `From<T> for T` :(
    pub fn id_into<Into: From<Id>>(self) -> PadEvent<Into> {
        match self {
            Self::Added => PadEvent::Added,
            Self::Removed => PadEvent::Removed,
            Self::Group { group, event } => PadEvent::Group {
                group: Into::from(group),
                event,
            },
        }
    }
}
/// Events associated with a specific `Pad`.
#[derive(Clone, Copy, Debug)]
pub enum PadGroupEvent {}
#[derive(Clone, Debug)]
pub enum Event<Id> {
    Tool { tool: Id, event: ToolEvent<Id> },
    Tablet { tablet: Id, event: TabletEvent },
    Pad { pad: Id, event: PadEvent<Id> },
}
impl<Id> Event<Id> {
    // Can't impl `From`, due to conflict with `From<T> for T` :(
    pub fn id_into<Into: From<Id>>(self) -> Event<Into> {
        match self {
            Self::Tool { tool, event } => Event::Tool {
                tool: Into::from(tool),
                event: event.id_into::<Into>(),
            },
            Self::Tablet { tablet, event } => Event::Tablet {
                tablet: Into::from(tablet),
                event,
            },
            Self::Pad { pad, event } => Event::Pad {
                pad: Into::from(pad),
                event: event.id_into::<Into>(),
            },
        }
    }
}
