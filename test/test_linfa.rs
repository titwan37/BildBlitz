use linfa::prelude::*;
use linfa_clustering::Dbscan;
use ndarray::Array2;

fn main() {
    let data = Array2::<f64>::zeros((10, 5));
    let dataset = DatasetBase::from(data);
    
    let clustered = Dbscan::params(3)
        .tolerance(0.5)
        .transform(dataset)
        .unwrap();
    
    for (i, c) in clustered.targets().iter().enumerate() {
        println!("{:?}", c);
    }
}
