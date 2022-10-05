use argmin::core::ArgminKV;
use argmin::core::IterState;
use argmin::core::Observe;
use argmin::core::ObserverMode;
use argmin::prelude::ArgminOp;
use argmin::prelude::Error;
use argmin::prelude::Executor;
use argmin::solver::neldermead::NelderMead;
use argmin::solver::particleswarm::ParticleSwarm;
use argmin::solver::simulatedannealing::SATempFunc;
use argmin::solver::simulatedannealing::SimulatedAnnealing;
use friedrich::gaussian_process::GaussianProcess;
use friedrich::kernel::SquaredExp;
use friedrich::prior::ConstantPrior;
use libm::erf;
use rand::Rng;
use std::borrow::Borrow;
use std::f64::consts::PI;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::rc::Rc;

struct Problem {
    gp: GaussianProcess<SquaredExp, ConstantPrior>,
    best: f64,
    xi: f64,
}

fn normal_pdf(x: f64, mean: f64, var: f64) -> f64 {
    let dx = x - mean;
    (-0.5 * dx * dx / var).exp() / (2.0 * PI * var).sqrt()
}

fn normal_cdf(x: f64, mean: f64, var: f64) -> f64 {
    0.5 * (1.0 + erf((x - mean) / (2.0 * var).sqrt()))
}

impl ArgminOp for Problem {
    /// Type of the parameter vector
    type Param = Vec<f64>;
    /// Type of the return value computed by the cost function
    type Output = f64;
    /// Type of the Hessian. Can be `()` if not needed.
    //type Hessian = Vec<Vec<f64>>;
    type Hessian = ();
    type Jacobian = ();
    type Float = f64;

    /// Apply the cost function to a parameter `p`
    fn apply(&self, p: &Self::Param) -> Result<Self::Output, argmin::core::Error> {
        // http://krasserm.github.io/2018/03/21/bayesian-optimization/
        let mean = self.gp.predict(p);
        let var = self.gp.predict_variance(p);
        //println!("mean = {}, var = {}", mean, var);
        let stddev = var.sqrt();
        let z = (-mean + self.best - self.xi) / stddev;
        //println!("z = {}", z);
        let cdf = normal_cdf(z, 0.0, 1.0);
        let pdf = normal_pdf(z, 0.0, 1.0);
        //println!("cdf = {}, pdf = {}", cdf, pdf);
        let result = -((-mean + self.best - self.xi) * cdf + stddev * pdf);
        //println!("ArgminOp::apply returning {}", result);
        Ok(result)
    }

    // /// Compute the gradient at parameter `p`.
    // fn gradient(&self, p: &Self::Param) -> Result<Self::Param, Error> {
    //     Ok(rosenbrock_2d_derivative(p, self.a, self.b))
    // }

    // /// Compute the Hessian at parameter `p`.
    // fn hessian(&self, p: &Self::Param) -> Result<Self::Hessian, Error> {
    //     let t = rosenbrock_2d_hessian(p, self.a, self.b);
    //     Ok(vec![vec![t[0], t[1]], vec![t[2], t[3]]])
    // }
}

pub trait Function: Debug {
    fn evaluate(&self, params: &[f64]) -> f64;
    fn modify(&self, params: &mut [f64], extent: f64);
}

#[derive(Debug)]
struct FunctionArgmin {
    f: Rc<Function>,
}

impl ArgminOp for FunctionArgmin {
    /// Type of the parameter vector
    type Param = Vec<f64>;
    /// Type of the return value computed by the cost function
    type Output = f64;
    /// Type of the Hessian. Can be `()` if not needed.
    //type Hessian = Vec<Vec<f64>>;
    type Hessian = ();
    type Jacobian = ();
    type Float = f64;

    /// Apply the cost function to a parameter `p`
    fn apply(&self, p: &Self::Param) -> Result<Self::Output, argmin::core::Error> {
        println!("Inside FunctionArgmin::apply"); // TODO: remove
        Ok(self.f.evaluate(p))
    }

    fn modify(&self, param: &Self::Param, extent: Self::Float) -> Result<Self::Param, Error> {
        let mut params = param.clone();
        self.f.modify(&mut params, extent);
        Ok(params)
    }
}

pub struct Optimizer {
    inputs: Vec<Vec<f64>>,
    outputs: Vec<f64>,
    function: Box<dyn Function>,
    lower: Vec<f64>,
    upper: Vec<f64>,
    initial: Vec<f64>,
}

impl Optimizer {
    pub fn new(
        function: Box<dyn Function>,
        lower: Vec<f64>,
        upper: Vec<f64>,
        initial: Vec<f64>,
    ) -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            function,
            lower,
            upper,
            initial,
        }
    }

    pub fn optimize(&mut self) -> Result<(), argmin::core::Error> {
        let mut rng = rand::thread_rng();
        let mut best = None;
        for _ in 0..self.lower.len() + 1 {
            let inputs: Vec<_> = self
                .lower
                .iter()
                .zip(self.upper.iter())
                .map(|(lo, hi)| lo + rng.gen::<f64>() * (hi - lo))
                .collect();
            let value = self.function.evaluate(&inputs);
            match best {
                None => best = Some(value),
                Some(b) => {
                    if b > value {
                        best = Some(value);
                    }
                }
            }
            eprintln!("{:?} {}", inputs, value);
            self.outputs.push(value);
            self.inputs.push(inputs);
        }
        let mut best = best.unwrap();
        let mut bestParam = self.initial.clone();
        // { // TODO: remove
        //     let gp = GaussianProcess::default(self.inputs.clone(), self.outputs.clone());
        //     let gp2 = GaussianProcess::default(self.inputs.clone(), self.outputs.clone());
        //     let cost_function = Problem {
        //         gp,
        //         best,
        //         xi: 0.01,
        //     };
        //     for i in 0..100 {
        //         let x = (i as f64) / 100.0 * 20.0 - 10.0;
        //         let y: f64 = cost_function.apply(&vec![x])?;
        //         println!("{} {} {}", x, y, gp2.predict(&vec![x]));
        //     }
        //     std::process::exit(0);
        // }
        loop {
            let gp = GaussianProcess::default(self.inputs.clone(), self.outputs.clone());
            // let gp = GaussianProcess::builder(self.inputs.clone(), self.outputs.clone())
            //     //.set_prior(ConstantPrior::new(best))
            //     .train();
            println!("GP noise: {}, prior: {:?}", gp.noise, gp.prior);
            let cost_function = Problem { gp, best, xi: 0.01 };

            let solver =
                ParticleSwarm::new((self.lower.clone(), self.upper.clone()), 10, 0.5, 0.0, 0.5)?;
            // let solver = SimulatedAnnealing::new(15.0)?
            //     // Optional: Define temperature function (defaults to `SATempFunc::TemperatureFast`)
            //     .temp_func(SATempFunc::Boltzmann)
            //     /////////////////////////
            //     // Stopping criteria   //
            //     /////////////////////////
            //     // Optional: stop if there was no new best solution after 1000 iterations
            //     .stall_best(100)
            //     // Optional: stop if there was no accepted solution after 1000 iterations
            //     .stall_accepted(100)
            //     /////////////////////////
            //     // Reannealing         //
            //     /////////////////////////
            //     // Optional: Reanneal after 1000 iterations (resets temperature to initial temperature)
            //     .reannealing_fixed(100)
            //     // Optional: Reanneal after no accepted solution has been found for `iter` iterations
            //     .reannealing_accepted(50)
            //     // Optional: Start reannealing after no new best solution has been found for 800 iterations
            //             .reannealing_best(80);

            let executor =
                Executor::new(cost_function, solver, self.initial.clone()).max_iters(1000);
            let res = executor.run()?;
            let inputs = res.state().best_param.clone();
            let value = self.function.evaluate(&inputs);
            if value < best {
                //best = value;
                best = self.function.evaluate(&inputs);
                bestParam = inputs.clone();
            }
            println!("best = {}, {:?}", best, bestParam);
            self.inputs.push(inputs);
            self.outputs.push(value);
            // Print Result
            println!("{}", res);
        }
    }
}

struct PrintObserver<O: ArgminOp> {
    phantom: PhantomData<O>,
}

impl<O: ArgminOp> Default for PrintObserver<O> {
    fn default() -> Self {
        Self {
            phantom: PhantomData,
        }
    }
}

impl<O: ArgminOp + Debug> Observe<O> for PrintObserver<O>
where
    IterState<O>: Debug,
{
    fn observe_iter(&mut self, state: &IterState<O>, kv: &ArgminKV) -> Result<(), Error> {
        println!("state = {:?}, kv = {:?}", state, kv);
        Ok(())
    }
}

pub struct Optimizer2 {
    inputs: Vec<Vec<f64>>,
    outputs: Vec<f64>,
    function: Rc<dyn Function>,
    lower: Vec<f64>,
    upper: Vec<f64>,
    initial: Vec<f64>,
}

impl Optimizer2 {
    pub fn new(
        function: Rc<dyn Function>,
        lower: Vec<f64>,
        upper: Vec<f64>,
        initial: Vec<f64>,
    ) -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            function,
            lower,
            upper,
            initial,
        }
    }

    pub fn optimize(&mut self) -> Result<(), argmin::core::Error> {
        let cost_function = FunctionArgmin {
            f: self.function.clone(),
        };
        //let solver = ParticleSwarm::new((self.lower.clone(), self.upper.clone()), 10, 0.5, 0.0, 0.5)?;

        let solver = SimulatedAnnealing::new(1.0)?
            // Optional: Define temperature function (defaults to `SATempFunc::TemperatureFast`)
            .temp_func(SATempFunc::Boltzmann)
            /////////////////////////
            // Stopping criteria   //
            /////////////////////////
            // Optional: stop if there was no new best solution after 1000 iterations
            .stall_best(100)
            // Optional: stop if there was no accepted solution after 1000 iterations
            .stall_accepted(100)
            /////////////////////////
            // Reannealing         //
            /////////////////////////
            // Optional: Reanneal after 1000 iterations (resets temperature to initial temperature)
            .reannealing_fixed(100)
            // Optional: Reanneal after no accepted solution has been found for `iter` iterations
            .reannealing_accepted(50)
            // Optional: Start reannealing after no new best solution has been found for 800 iterations
            .reannealing_best(80);

        let executor = Executor::new(cost_function, solver, self.initial.clone())
            .max_iters(1000)
            .add_observer(PrintObserver::default(), ObserverMode::Always);
        let res = executor.run()?;
        Ok(())
    }
}
