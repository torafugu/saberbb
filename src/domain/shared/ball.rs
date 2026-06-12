pub enum TrajectoryType {
    Grounder,
    Liner,
    Fly,
    PopUp,
}

pub struct Ball {
    pub launch_speed: f32, // km/h
    pub launch_angle: f32, // Y arc degree
    pub spray_angle: f32,  // X arc degree
    pub distance: f32,     // m
    pub hang_time: f32,    // second
    pub trajectory: TrajectoryType,
}

impl Ball {
    pub fn batted(&mut self, distance: f32, hang_time: f32) {
        self.distance = distance;
        self.hang_time = hang_time;
    }
}
