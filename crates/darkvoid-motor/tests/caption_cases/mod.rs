use serde_json::{Value, json};

pub fn captions(mode: &str, seconds: u32) -> Value {
    let cues: Vec<_> = (0..seconds / 3)
        .map(|i| {
            let start = f64::from(i * 3);
            json!({"start":start,"end":start+3.0,"words":[
                {"text":"Olá,","start":start,"end":start+0.8},
                {"text":"São","start":start+1.0,"end":start+1.8},
                {"text":"Paulo!","start":start+2.0,"end":start+2.9}
            ]})
        })
        .collect();
    json!({"cues":cues,"style":{
        "mode":mode,"font":"Inter","weight":700,"size":54,"color":"#FFFFFF","activeColor":"#FF3355",
        "outline":{"width":2,"color":"#000000"},
        "shadow":{"blur":3,"offsetY":3,"color":"#000000","opacity":0.6},
        "box":{"color":"#000000","opacity":0.5,"padding":12,"radius":10},
        "uppercase":false,"position":{"x":0.5,"y":0.85},"maxWidth":0.8
    }})
}
