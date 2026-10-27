//! Validated, serializable inputs shared by the solver, files and UI.
use serde::{Deserialize, Serialize};

macro_rules! choices {
    ($name:ident { $($variant:ident => ($wire:literal, $label:literal)),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name { $(#[serde(rename = $wire)] $variant),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn label(self) -> &'static str { match self { $(Self::$variant => $label),+ } }
        }
    };
}
choices!(Planet {
    Earth => ("earth", "Земля · ISA"),
    Mars => ("mars", "Марс"),
    Venus => ("venus", "Венера"),
    Titan => ("titan", "Титан"),
    Water => ("water", "Вода"),
    Custom => ("custom", "Своя среда")
});
choices!(Profile {
    Naca4 => ("naca4", "NACA 4"),
    Naca5 => ("naca5", "NACA 230xx"),
    Supercritical => ("supercritical", "Сверхкритический · прибл."),
    Biconvex => ("biconvex", "Двояковыпуклый"),
    DoubleWedge => ("double_wedge", "Двойной клин"),
    Cylinder => ("cylinder", "Цилиндр"),
    FlatPlate => ("flat_plate", "Пластина"),
    Custom => ("custom", "Из файла")
});
choices!(Flap {
    Plain => ("plain", "Простой"),
    Split => ("split", "Щиток"),
    Slotted => ("slotted", "Щелевой"),
    Fowler => ("fowler", "Фаулер")
});
choices!(Engine {
    Turbofan => ("turbofan", "ТРДД"),
    Turbojet => ("turbojet", "ТРД"),
    Piston => ("piston_prop", "Поршневой"),
    Electric => ("electric_prop", "Электрический"),
    Rocket => ("rocket", "Ракетный")
});

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Simulation {
    pub planet: Planet,
    pub altitude: f64,
    pub velocity: f64,
    pub alpha: f64,
    pub custom_density: f64,
    pub custom_speed_of_sound: f64,
    pub profile_type: Profile,
    pub profile_name: String,
    pub camber: f64,
    pub camber_pos: f64,
    pub thickness: f64,
    pub cylinder_rpm: f64,
    pub flap_angle: f64,
    pub flap_type: Flap,
    pub slat_angle: f64,
    pub chord: f64,
    pub span: f64,
    pub sweep: f64,
    pub taper: f64,
    /// Negative tip twist is washout, consistently in geometry and aerodynamics.
    pub twist: f64,
    pub dihedral: f64,
    pub has_winglets: bool,
    pub engine_type: Engine,
    pub max_thrust: f64,
    /// Total wet mass, including fuel.
    pub aircraft_mass: f64,
    pub fuel_mass: f64,
    /// Fuel mass flow / thrust in kg/(N s).
    pub tsfc: f64,
    pub custom_upper: Option<Vec<Point>>,
    pub custom_lower: Option<Vec<Point>>,
}
impl Default for Simulation {
    fn default() -> Self {
        Self {
            planet: Planet::Earth,
            altitude: 1200.0,
            velocity: 65.0,
            alpha: 3.5,
            custom_density: 1.225,
            custom_speed_of_sound: 340.29,
            profile_type: Profile::Naca4,
            profile_name: "NACA 2412".into(),
            camber: 2.0,
            camber_pos: 40.0,
            thickness: 12.0,
            cylinder_rpm: 0.0,
            flap_angle: 0.0,
            flap_type: Flap::Plain,
            slat_angle: 0.0,
            chord: 1.6,
            span: 11.0,
            sweep: 0.0,
            taper: 0.68,
            twist: -1.8,
            dihedral: 2.0,
            has_winglets: false,
            engine_type: Engine::Piston,
            max_thrust: 2400.0,
            aircraft_mass: 1150.0,
            fuel_mass: 150.0,
            tsfc: 0.000018,
            custom_upper: None,
            custom_lower: None,
        }
    }
}
impl Simulation {
    pub fn area(&self) -> f64 {
        self.span * self.chord * (1.0 + self.taper) * 0.5
    }
    pub fn aspect_ratio(&self) -> f64 {
        self.span.powi(2) / self.area()
    }
    pub fn mean_chord(&self) -> f64 {
        self.chord * 2.0 / 3.0 * (1.0 + self.taper + self.taper.powi(2)) / (1.0 + self.taper)
    }

    /// Reject invalid files instead of silently feeding NaN or zero dimensions to solvers.
    pub fn validate(&self) -> Result<(), String> {
        let limits = [
            ("altitude", self.altitude, 0.0, 80000.0),
            ("velocity", self.velocity, 0.0, 1500.0),
            ("alpha", self.alpha, -30.0, 45.0),
            ("customDensity", self.custom_density, 0.00001, 2000.0),
            (
                "customSpeedOfSound",
                self.custom_speed_of_sound,
                1.0,
                3000.0,
            ),
            ("camber", self.camber, 0.0, 10.0),
            ("camberPos", self.camber_pos, 10.0, 90.0),
            ("thickness", self.thickness, 1.0, 35.0),
            ("cylinderRpm", self.cylinder_rpm, -10000.0, 10000.0),
            ("flapAngle", self.flap_angle, -20.0, 45.0),
            ("slatAngle", self.slat_angle, 0.0, 25.0),
            ("chord", self.chord, 0.1, 15.0),
            ("span", self.span, 0.2, 80.0),
            ("sweep", self.sweep, -20.0, 70.0),
            ("taper", self.taper, 0.1, 1.0),
            ("twist", self.twist, -15.0, 15.0),
            ("dihedral", self.dihedral, -15.0, 20.0),
            ("maxThrust", self.max_thrust, 0.0, 2000000.0),
            ("aircraftMass", self.aircraft_mass, 1.0, 1000000.0),
            ("fuelMass", self.fuel_mass, 0.0, self.aircraft_mass * 0.95),
            ("tsfc", self.tsfc, 0.0000001, 0.01),
        ];
        for (name, value, min, max) in limits {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!("{name}: требуется значение {min}…{max}"));
            }
        }
        if self.profile_name.chars().count() > 120 {
            return Err("Слишком длинное имя профиля".into());
        }
        if self.profile_type == Profile::Custom {
            for points in [&self.custom_upper, &self.custom_lower] {
                let points = points
                    .as_ref()
                    .ok_or("Нет координат пользовательского профиля")?;
                if !(6..=4096).contains(&points.len())
                    || points.iter().any(|p| {
                        !p.x.is_finite()
                            || !p.y.is_finite()
                            || !(0.0..=1.0).contains(&p.x)
                            || p.y.abs() > 2.0
                    })
                {
                    return Err("Недопустимые координаты профиля".into());
                }
                if points.windows(2).any(|p| p[0].x > p[1].x) {
                    return Err("Координаты должны идти от носка к хвосту".into());
                }
                if points.last().unwrap().x - points[0].x < 0.8 {
                    return Err("Поверхность не охватывает хорду".into());
                }
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct Preset {
    pub name: String,
    pub state: Simulation,
}
pub fn presets() -> Vec<Preset> {
    serde_json::from_str(include_str!("../../../assets/presets.json"))
        .expect("bundled presets are validated in tests")
}
