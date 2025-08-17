use std::collections::HashMap;
use std::io::Read; //to get read_to_string working again 

struct Todo{
    map : HashMap<String,bool>, 
}
impl Todo{
    //the insert function 
    fn insert(&mut self , key:String){
        self.map.insert(key,true);
    }
    //write content to file 
    fn save(self) -> Result<(),std::io::Error>{//returning either a unit or an error
        let mut content = String::new();
        for(k,v) in self.map{
            let record = format!("{}\t{}\n",k,v);
            content.push_str(&record);
        }
        std::fs::write("db.txt", content)
        }
    //new function
    fn new() -> Result<Todo, std::io::Error>{ //returning either a new todo or an error 
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true) //opens it if it already exits or creates it
            .open("db.txt")?;
        let mut content = String::new();
        f.read_to_string(&mut content)?; //read and append to content
        let mut map = HashMap::new();
        for i in content.lines(){
            let mut values = i.split('\t');
            let key = values.next().expect("No Key");
            let val = values.next().expect("No Value");
            map.insert(String::from(key), val == "true");
        }
        Ok(Todo { map })
    }
    fn complete(&mut self,key: &String) -> Option<()>{
        match self.map.get_mut(key){
            Some(v) => Some(*v = false),
            None => None,
        }
    }
    }
//the main block 
fn main(){
let action = std::env::args().nth(1).expect("Please specify an action");
let item = std::env::args().nth(2).expect("Please specify an item");
println!("{:?}, {:?}", action, item);
let mut todo = Todo::new().expect("Failed to create Todo instance");
if action == "add"{
    todo.insert(item);
    match todo.save(){
        Ok(_) => println!("Todo saved!"),
        Err(e) => println!("An error occured: {}", e),
    }
    }
    else if action == "complete"{
        match todo.complete(&item){
            None => println!("Item not found!"),
            Some(_) => match todo.save(){
                Ok(_) => println!("Todo completed!"),
                Err(e) => println!("An error occured: {}", e),
            },
        }
    }
}
