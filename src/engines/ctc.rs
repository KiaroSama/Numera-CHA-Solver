use crate::core::contracts::Error;

pub fn decode(
    output: &[f32],
    shape: &[i64],
    characters: &[String],
) -> Result<(String, f64), Error> {
    if shape.len() != 3
        || shape[0] != 1
        || shape[1] <= 0
        || shape[2] != characters.len() as i64
        || output.len() != shape[1] as usize * characters.len()
    {
        return Err(Error::new(
            "model_contract",
            "OCR output does not match its reviewed character map.",
        ));
    }
    if output
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0 || *v > 1.00001)
    {
        return Err(Error::new(
            "model_contract",
            "OCR output contains invalid probability values.",
        ));
    }
    let mut text = String::new();
    let mut previous = None;
    let mut scores = Vec::new();
    for row in output.chunks_exact(characters.len()) {
        let mut index = 0;
        for i in 1..row.len() {
            if row[i] > row[index] {
                index = i;
            }
        }
        if index != 0 && previous != Some(index) {
            text.push_str(&characters[index]);
            scores.push(row[index]);
        }
        previous = Some(index);
    }
    let confidence = if scores.is_empty() {
        0.0
    } else {
        (scores.iter().sum::<f32>() / scores.len() as f32) as f64
    };
    Ok((text, confidence))
}
