pub struct Team {
    name: String,
}

impl Team {
    pub fn new(name: &str) -> Team {
        Team {
            name: name.to_string(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone)]
pub struct Batter {
    name: String,
    pub average: f32,
}

impl Batter {
    pub fn new(name: &str, average: f32) -> Batter {
        Batter {
            name: name.to_string(),
            average: average,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn average(&self) -> f32 {
        self.average
    }
}
