const FOUL_DEGREE: f64 = 45.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrajectoryType {
    Grounder,
    Liner,
    Fly,
    PopUp,
}

pub struct Ball {
    pub launch_speed: f64, // km/h
    pub launch_angle: f64, // Z arc degree
    pub spray_angle: f64,  // X arc degree
    pub distance: f64,     // m
    pub hang_time: f64,    // second
    pub trajectory: TrajectoryType,
}

impl Ball {
    pub fn batted(&mut self, distance: f64, hang_time: f64) {
        self.distance = distance;
        self.hang_time = hang_time;
    }

    pub fn is_foul(&self) -> bool {
        if self.spray_angle.abs() <= FOUL_DEGREE {
            true
        } else {
            false
        }
    }
}
