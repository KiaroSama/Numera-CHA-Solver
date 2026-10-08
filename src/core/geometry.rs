use super::contracts::Error;

#[derive(Debug, Clone, Copy)]
pub struct Transform {
    pub width: f64,
    pub height: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub offset_x: f64,
    pub offset_y: f64,
}
impl Transform {
    pub fn validate_transform(&self) -> std::result::Result<(), Error> {
        let values = [
            self.width,
            self.height,
            self.scale_x,
            self.scale_y,
            self.offset_x,
            self.offset_y,
        ];
        if values.iter().any(|v| !v.is_finite()) || values[..4].iter().any(|v| *v <= 0.0) {
            return Err(Error::new(
                "invalid_geometry",
                "Dimensions and scale must be finite and positive.",
            ));
        }
        Ok(())
    }
    pub fn validate(&self, x: f64, y: f64) -> std::result::Result<(), Error> {
        self.validate_transform()?;
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= self.width
            || y >= self.height
        {
            return Err(Error::new(
                "invalid_geometry",
                "Action is outside the authorized image region.",
            ));
        }
        Ok(())
    }
    pub fn to_viewport(&self, x: f64, y: f64) -> std::result::Result<(f64, f64), Error> {
        self.validate(x, y)?;
        Ok((
            x * self.scale_x + self.offset_x,
            y * self.scale_y + self.offset_y,
        ))
    }
    pub fn to_image(&self, x: f64, y: f64) -> std::result::Result<(f64, f64), Error> {
        self.validate_transform()?;
        let point = (
            (x - self.offset_x) / self.scale_x,
            (y - self.offset_y) / self.scale_y,
        );
        self.validate(point.0, point.1)?;
        Ok(point)
    }
}
pub fn validate_revision(expected: &str, actual: &str) -> std::result::Result<(), Error> {
    if expected.is_empty() || expected != actual {
        return Err(Error::new(
            "stale_observation",
            "Image revision changed; observe and recognize again.",
        ));
    }
    Ok(())
}
