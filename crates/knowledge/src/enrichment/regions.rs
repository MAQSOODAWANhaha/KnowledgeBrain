//! Bounded OCR subdivision. Independent overlap-strip OCR proves whether text
//! belongs to two overlapping crops; matching text alone is never deduplicated.
use image::{DynamicImage, GenericImageView};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Fragment {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub text: String,
    pub complete: bool,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Recognition {
    pub text: String,
    pub complete: bool,
    pub physical_calls: usize,
    pub fragments: Vec<Fragment>,
    pub overlap_fragments: Vec<Fragment>,
}
struct Plan {
    width: u32,
    todo: Vec<(u32, u32, u32)>,
    fragments: Vec<Fragment>,
    calls: usize,
    max_depth: u32,
    max_calls: usize,
}
impl Plan {
    fn new(image: &DynamicImage, max_depth: u32, max_calls: usize) -> Self {
        let (width, height) = image.dimensions();
        let valid = width > 0 && height > 0;
        Self {
            width,
            todo: if valid { vec![(0, height, 0)] } else { vec![] },
            fragments: if valid {
                vec![]
            } else {
                vec![Fragment {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                    text: String::new(),
                    complete: false,
                    reason: Some("invalid empty OCR image geometry".into()),
                }]
            },
            calls: 0,
            max_depth,
            max_calls,
        }
    }
    fn crop(&mut self, image: &DynamicImage, top: u32, bottom: u32) -> Result<Vec<u8>, String> {
        if top >= bottom || bottom > image.height() || self.width == 0 {
            return Err("invalid OCR region geometry".into());
        }
        if self.calls >= self.max_calls {
            return Err("OCR physical call budget exhausted".into());
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        image
            .crop_imm(0, top, self.width, bottom - top)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(|e| format!("encode OCR region: {e}"))?;
        self.calls += 1;
        Ok(bytes.into_inner())
    }
    fn next(&mut self, image: &DynamicImage) -> Option<(u32, u32, u32, Vec<u8>)> {
        while let Some((top, bottom, depth)) = self.todo.pop() {
            match self.crop(image, top, bottom) {
                Ok(bytes) => return Some((top, bottom, depth, bytes)),
                Err(error) => self.accept(top, bottom, depth, Err(error)),
            }
        }
        None
    }
    fn fragment(&self, top: u32, bottom: u32, result: Result<String, String>) -> Fragment {
        match result {
            Ok(text) => Fragment {
                left: 0,
                top,
                right: self.width,
                bottom,
                text,
                complete: true,
                reason: None,
            },
            Err(reason) => Fragment {
                left: 0,
                top,
                right: self.width,
                bottom,
                text: String::new(),
                complete: false,
                reason: Some(reason),
            },
        }
    }
    fn accept(&mut self, top: u32, bottom: u32, depth: u32, result: Result<String, String>) {
        match result {
            Err(ref error)
                if (error.contains("length") || error.contains("max_tokens"))
                    && depth < self.max_depth
                    && bottom - top >= 64 =>
            {
                let mid = top + (bottom - top) / 2;
                let overlap = ((bottom - top) / 20).clamp(1, 32);
                self.todo.push((mid - overlap, bottom, depth + 1));
                self.todo.push((top, mid + overlap, depth + 1));
            }
            result => self.fragments.push(self.fragment(top, bottom, result)),
        }
    }
    fn boundary(&self, index: usize) -> (u32, u32) {
        let left = &self.fragments[index - 1];
        let right = &self.fragments[index];
        (left.top.max(right.top), left.bottom.min(right.bottom))
    }
    fn finish(mut self, boundaries: Vec<Fragment>) -> Recognition {
        let mut text = self
            .fragments
            .first()
            .filter(|f| f.complete)
            .map(|f| f.text.clone())
            .unwrap_or_default();
        for index in 1..self.fragments.len() {
            let (previous, current) = self.fragments.split_at_mut(index);
            let previous = &previous[index - 1];
            let current = &mut current[0];
            let boundary = &boundaries[index - 1];
            if !previous.complete || !current.complete || !boundary.complete {
                current.complete = false;
                current.reason = Some("incomplete spatial boundary evidence".into());
                continue;
            }
            let shared = &boundary.text;
            if shared.is_empty() {
                // Blank physical overlap proves equal strings elsewhere are
                // separate source occurrences and both must be retained.
                if !text.is_empty()
                    && !current.text.is_empty()
                    && !text.ends_with('\n')
                    && !current.text.starts_with('\n')
                {
                    text.push('\n');
                }
                text.push_str(&current.text);
                continue;
            }
            // No trimming, punctuation normalization or semantic stitching.
            let unique_previous = previous.text.matches(shared).count() == 1;
            let unique_current = current.text.matches(shared).count() == 1;
            if !unique_previous
                || !unique_current
                || !previous.text.ends_with(shared)
                || !current.text.starts_with(shared)
            {
                current.complete = false;
                current.reason=Some("ambiguous OCR overlap: physical boundary does not uniquely match crop edges; manual review required".into());
                continue;
            }
            text.push_str(&current.text[shared.len()..]);
        }
        Recognition {
            text,
            complete: self.fragments.iter().all(|f| f.complete)
                && boundaries.iter().all(|f| f.complete),
            physical_calls: self.calls,
            fragments: self.fragments,
            overlap_fragments: boundaries,
        }
    }
}
pub fn recognize(
    image: &DynamicImage,
    max_depth: u32,
    max_calls: usize,
    mut complete: impl FnMut(&[u8]) -> Result<String, String>,
) -> Recognition {
    let mut plan = Plan::new(image, max_depth, max_calls);
    while let Some((top, bottom, depth, bytes)) = plan.next(image) {
        plan.accept(top, bottom, depth, complete(&bytes));
    }
    let mut boundaries = Vec::new();
    for index in 1..plan.fragments.len() {
        let (top, bottom) = plan.boundary(index);
        let result = plan
            .crop(image, top, bottom)
            .and_then(|bytes| complete(&bytes));
        boundaries.push(plan.fragment(top, bottom, result));
    }
    plan.finish(boundaries)
}
pub async fn recognize_async<F, Fut>(
    image: &DynamicImage,
    max_depth: u32,
    max_calls: usize,
    mut complete: F,
) -> Recognition
where
    F: FnMut(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let mut plan = Plan::new(image, max_depth, max_calls);
    while let Some((top, bottom, depth, bytes)) = plan.next(image) {
        let result = complete(bytes).await;
        plan.accept(top, bottom, depth, result);
    }
    let mut boundaries = Vec::new();
    for index in 1..plan.fragments.len() {
        let (top, bottom) = plan.boundary(index);
        let result = match plan.crop(image, top, bottom) {
            Ok(bytes) => complete(bytes).await,
            Err(error) => Err(error),
        };
        boundaries.push(plan.fragment(top, bottom, result));
    }
    plan.finish(boundaries)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subdivisions_keep_order_and_independent_overlap_provenance() {
        let image = DynamicImage::new_rgb8(100, 200);
        let mut calls = 0;
        let result = recognize(&image, 2, 4, |_| {
            calls += 1;
            match calls {
                1 => Err("incomplete length".into()),
                2 => Ok("first\nshared".into()),
                3 => Ok("shared\nlast".into()),
                _ => Ok("shared".into()),
            }
        });
        assert!(result.complete);
        assert_eq!(result.text, "first\nshared\nlast");
        assert_eq!(result.physical_calls, 4);
        assert_eq!(result.overlap_fragments[0].text, "shared");
        assert_eq!(result.fragments[1].text, "shared\nlast");
        let partial = recognize(&image, 3, 2, |_| Err("incomplete length".into()));
        assert!(!partial.complete);
        assert_eq!(partial.physical_calls, 2);
    }
    #[test]
    fn identical_lines_outside_blank_overlap_are_both_preserved() {
        let mut calls = 0;
        let result = recognize(&DynamicImage::new_rgb8(100, 200), 2, 4, |_| {
            calls += 1;
            match calls {
                1 => Err("length".into()),
                2 | 3 => Ok("签名".into()),
                _ => Ok(String::new()),
            }
        });
        assert!(result.complete);
        assert_eq!(result.text, "签名\n签名");
    }
    #[test]
    fn zero_geometry_never_calls_provider() {
        let result = recognize(&DynamicImage::new_rgb8(0, 0), 2, 3, |_| {
            panic!("unexpected call")
        });
        assert!(!result.complete);
        assert_eq!(result.physical_calls, 0);
    }
    #[tokio::test]
    async fn ambiguous_physical_boundary_blocks() {
        let mut calls = 0;
        let result = recognize_async(&DynamicImage::new_rgb8(100, 200), 2, 4, |_| {
            calls += 1;
            let n = calls;
            async move {
                match n {
                    1 => Err("length".into()),
                    2 => Ok("first\nedge".into()),
                    3 => Ok("different\nlast".into()),
                    _ => Ok("edge".into()),
                }
            }
        })
        .await;
        assert!(!result.complete);
        assert_eq!(result.physical_calls, 4);
        assert!(
            result.fragments[1]
                .reason
                .as_deref()
                .unwrap()
                .contains("ambiguous")
        );
    }
}
