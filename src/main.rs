use rand::Rng;
const MAX_OUT: i32 = 3;

fn main() {
    let _avg :f64 = 0.3;
    let mut out_count = 0;
    let mut _batting_first_scores: Vec<i32> = Vec::new();
    let mut _field_first_scores: Vec<i32> = Vec::new();
    let mut is_first_runner: bool = false;
    let mut is_second_runner: bool = false;
    let mut is_third_runner: bool = false;

    let mut rng = rand::thread_rng();

    while out_count < MAX_OUT {
        let trial: f64 = rng.gen();
        println!("Trial: {trial}");

        if _avg > trial {
            println!("Hit!");

            if is_second_runner {
                is_third_runner = true;
            }
            if is_first_runner {
                is_second_runner = true;
            }
            is_first_runner = true;
            
        } else {
            println!("Out!");
            if out_count < MAX_OUT {
                out_count += 1;
            }
        }
    
        println!("Score: 0-0");
        println!("  <{}>", runner_text(is_second_runner));
        println!("<{}> <{}>", runner_text(is_third_runner), runner_text(is_first_runner));
        println!("  <H>");
        println!("Out Count: {out_count}");
    }



    

       
}

fn runner_text(runner: bool) -> &'static str {
    if runner {
        "R"
    } else {
        "-"
    }
}