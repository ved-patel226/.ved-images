use image::{ GenericImageView, Pixel, Rgba, RgbaImage };
use std::fs::File;
use std::io::{ Write, BufReader, BufRead };
use std::collections::HashMap;
use rayon::prelude::*;

//ANCHOR - Encode
// Read an image file and encode it into a .ved file.
/* 
  ┌────────────────────────────────────────────────────────────────────────────┐
  │ The .ved file format is as follows:                                        │
  │ 1. The first line contains the image dimensions in the format              │
  │ "width,height".                                                            │
  │ 2. The second line contains a list of frequently used colors in the        │
  │ format                                                                     │
  │ "index=color".                                                             │
  │ 3. Each subsequent line contains a row of the image, where each pixel is   │
  │ represented by an index or a                                               │
  │ color.                                                                     │
  │ 4. Pixels with the same color are represented by an empty string.          │
  │ 5. Pixels with a color not in the frequently used colors list are          │
  │ represented by the color                                                   │
  │ itself.                                                                    │
  │ 6. The image is encoded using run-length encoding.                         │
  │ 7. The output file is named "output.ved".                                  │
  │                                                                            │
  └────────────────────────────────────────────────────────────────────────────┘
 */
#[allow(dead_code)]
fn encode() -> Result<(), Box<dyn std::error::Error>> {
    let img = image::open("image.png")?;
    let (width, height) = img.dimensions();

    // Process rows in parallel.
    let row_results: Vec<(String, HashMap<String, u32>)> = (0..height)
        .into_par_iter()
        .map(|y| {
            let mut colors = Vec::with_capacity(width as usize);
            let mut local_count = HashMap::new();
            for x in 0..width {
                let pixel = img.get_pixel(x, y);
                let channels = pixel.channels();
                let color = format!("{:02X}{:02X}{:02X}", channels[0], channels[1], channels[2]);
                *local_count.entry(color.clone()).or_insert(0) += 1;
                colors.push(color);
            }
            (colors.join(","), local_count)
        })
        .collect();

    // Merge row strings and local pixel counts.
    let mut new_img = Vec::with_capacity(row_results.len());
    let mut pixel_count = HashMap::new();
    for (row, local_count) in row_results {
        new_img.push(row);
        for (color, count) in local_count {
            *pixel_count.entry(color).or_insert(0) += count;
        }
    }

    let mut img_output = Vec::new();
    // First line: image dimensions.
    img_output.push(format!("{},{}", width, height));

    // Build a mapping for frequently used colors.
    let mut counts: Vec<(&String, &u32)> = pixel_count.iter().collect();
    counts.sort_by(|a, b| b.1.cmp(a.1));

    let mut variables = HashMap::new();
    let var_line = counts
        .into_iter()
        .filter(|&(_, &amount)| amount >= 2)
        .enumerate()
        .map(|(i, (color, _))| {
            variables.insert(color.clone(), i);
            format!("{}={}", i, color)
        })
        .collect::<Vec<String>>()
        .join(",");
    img_output.push(var_line);

    // Process rows in parallel for run-length encoding.
    let encoded_rows: Vec<String> = new_img
        .into_par_iter()
        .map(|row| {
            let mut last_hex = String::new();
            let mut new_row = Vec::new();
            for hex in row.split(',') {
                if hex == last_hex {
                    new_row.push("".to_string());
                } else {
                    last_hex = hex.to_string();
                    if let Some(index) = variables.get(hex) {
                        new_row.push(index.to_string());
                    } else {
                        new_row.push(hex.to_string());
                    }
                }
            }
            new_row.join(",")
        })
        .collect();

    img_output.extend(encoded_rows);

    let mut file = File::create("output.ved")?;
    for line in img_output {
        writeln!(file, "{}", line)?;
    }

    println!(
        ".ved File size: {} bytes \n.png file size: {} bytes",
        file.metadata()?.len(),
        File::open("image.png")?.metadata()?.len()
    );

    Ok(())
}

//ANCHOR - Decode
// Read a .ved file and decode it into an .png file.
#[allow(dead_code)]
fn decode() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open("output.ved")?;
    let mut lines = BufReader::new(file).lines();

    let dimensions = lines.next().ok_or("Missing dimensions")??;
    let (width, height): (u32, u32) = {
        let dims: Vec<u32> = dimensions
            .split(',')
            .map(|s| s.parse::<u32>().unwrap())
            .collect();
        (dims[0], dims[1])
    };

    let mut img = RgbaImage::new(width, height);

    let variables_line = lines.next().ok_or("Missing variables line")??;
    let mut variables = HashMap::new();

    // for var in variables_line.split(',') {
    //     let parts: Vec<&str> = var.split('=').collect();
    //     if parts.len() == 2 {
    //         variables.insert(parts[0].parse::<usize>()?, parts[1].to_string());
    //     }
    // }

    variables_line.split(',').for_each(|var| {
        let parts: Vec<&str> = var.split('=').collect();
        if parts.len() == 2 {
            variables.insert(parts[0].parse::<usize>().unwrap(), parts[1].to_string());
        }
    });

    // Collect all remaining lines into a vector.
    let rows: Vec<String> = lines.collect::<Result<_, _>>()?;
    // Process each row in parallel.
    let decoded_rows: Vec<(usize, Vec<Rgba<u8>>)> = rows
        .par_iter()
        .enumerate()
        .map(|(y, row)| {
            let mut local_last_hex = String::new();
            let pixels = row
                .split(',')
                .map(|token| {
                    let token = if token == "" {
                        local_last_hex.clone()
                    } else {
                        local_last_hex = token.to_string();
                        local_last_hex.clone()
                    };
                    let color_str = variables
                        .get(&token.parse::<usize>().unwrap_or(usize::MAX))
                        .map(|s| s.as_str())
                        .unwrap_or(&token);
                    let color_str = if !color_str.starts_with('#') {
                        format!("#{}", color_str)
                    } else {
                        color_str.to_string()
                    };
                    if color_str.len() >= 7 {
                        let r = u8::from_str_radix(&color_str[1..3], 16).unwrap_or(0);
                        let g = u8::from_str_radix(&color_str[3..5], 16).unwrap_or(0);
                        let b = u8::from_str_radix(&color_str[5..7], 16).unwrap_or(0);
                        Rgba([r, g, b, 255])
                    } else {
                        println!("Invalid color: {}", color_str);
                        Rgba([0, 0, 0, 255])
                    }
                })
                .collect();
            (y, pixels)
        })
        .collect();

    // Write decoded pixels into the image.
    // Note: decoded_rows may be in any order, so sort by row index.
    let mut sorted_rows = decoded_rows;
    sorted_rows.sort_by_key(|&(y, _)| y);
    for (y, row_pixels) in sorted_rows {
        for (x, pixel) in row_pixels.into_iter().enumerate() {
            img.put_pixel(x as u32, y as u32, pixel);
        }
    }

    img.save("decoded.png")?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    encode()?;
    // decode()?;

    Ok(())
}
