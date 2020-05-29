use super::{generate_stream, generate_try_stream};
use futures::{pin_mut, stream::StreamExt, Stream};
use std::cell::Cell;
use std::rc::Rc;

#[tokio::test]
async fn noop_stream() {
    let s = generate_stream::<u32, _, _>(|_| async {});
    pin_mut!(s);

    while let Some(_) = s.next().await {
        unreachable!();
    }
}

#[tokio::test]
async fn empty_stream() {
    let mut ran = false;

    {
        let r = &mut ran;
        let s = generate_stream::<u32, _, _>(|_| async {
            *r = true;
            println!("hello world!");
        });
        pin_mut!(s);

        while let Some(_) = s.next().await {
            unreachable!();
        }
    }

    assert!(ran);
}

#[tokio::test]
async fn yield_single_value() {
    let s = generate_stream(|mut y| async move {
        y.send("hello").await;
    });

    let values: Vec<_> = s.collect().await;

    assert_eq!(1, values.len());
    assert_eq!("hello", values[0]);
}

#[tokio::test]
async fn yield_multi_value() {
    let s = generate_stream(|mut y| async move {
        y.send("hello").await;
        y.send("world").await;
        y.send("dizzy").await;
    });

    let values: Vec<_> = s.collect().await;

    assert_eq!(3, values.len());
    assert_eq!("hello", values[0]);
    assert_eq!("world", values[1]);
    assert_eq!("dizzy", values[2]);
}

#[tokio::test]
async fn return_stream() {
    fn build_stream() -> impl Stream<Item = u32> {
        generate_stream(|mut y| async move {
            y.send(1).await;
            y.send(2).await;
            y.send(3).await;
        })
    }

    let s = build_stream();

    let values: Vec<_> = s.collect().await;
    assert_eq!(3, values.len());
    assert_eq!(1, values[0]);
    assert_eq!(2, values[1]);
    assert_eq!(3, values[2]);
}

#[tokio::test]
async fn consume_channel() {
    use tokio::sync::mpsc;

    let (mut tx, mut rx) = mpsc::channel(10);

    let s = generate_stream(|mut y| async move {
        while let Some(v) = rx.recv().await {
            y.send(v).await;
        }
    });

    pin_mut!(s);

    for i in 0..3 {
        tx.send(i).await.unwrap();
        assert_eq!(Some(i), s.next().await);
    }

    drop(tx);
    assert_eq!(None, s.next().await);
}

#[tokio::test]
async fn borrow_self() {
    struct Data(String);

    impl Data {
        fn stream<'a>(&'a self) -> impl Stream<Item = &str> + 'a {
            generate_stream(move |mut y| async move {
                y.send(&self.0[..]).await;
            })
        }
    }

    let data = Data("hello".to_string());
    let s = data.stream();
    pin_mut!(s);

    assert_eq!(Some("hello"), s.next().await);
}

#[tokio::test]
async fn stream_in_stream() {
    let s = generate_stream(|mut y| async move {
        let s = generate_stream(|mut y2| async move {
            for i in 0..3 {
                y2.send(i).await;
            }
        });

        pin_mut!(s);
        while let Some(v) = s.next().await {
            y.send(v).await;
        }
    });

    let values: Vec<_> = s.collect().await;
    assert_eq!(3, values.len());
}

#[tokio::test]
async fn single_err() {
    let s = generate_try_stream(|mut y| async move {
        if true {
            return Err("hello");
        } else {
            y.send(Ok("world")).await;
        }

        unreachable!();
    });

    let values: Vec<_> = s.collect().await;
    assert_eq!(1, values.len());
    assert_eq!(Err("hello"), values[0]);
}

#[tokio::test]
async fn yield_then_err() {
    async fn failing() -> Result<(), &'static str> {
        Err("world")
    }

    let s = generate_try_stream(|mut y| async move {
        y.send(Ok("hello")).await;
        failing().await?;
        unreachable!();
    });

    let values: Vec<_> = s.collect().await;
    assert_eq!(2, values.len());
    assert_eq!(Ok("hello"), values[0]);
    assert_eq!(Err("world"), values[1]);
}

#[tokio::test]
async fn convert_err() {
    struct ErrorA(u8);
    #[derive(PartialEq, Debug)]
    struct ErrorB(u8);
    impl From<ErrorA> for ErrorB {
        fn from(a: ErrorA) -> ErrorB {
            ErrorB(a.0)
        }
    }
    async fn failing1() -> Result<(), ErrorA> {
        Err(ErrorA(1))
    }
    async fn failing2() -> Result<(), ErrorB> {
        Err(ErrorB(2))
    }

    fn test() -> impl Stream<Item = Result<&'static str, ErrorB>> {
        generate_try_stream(|mut y| async move {
            if true {
                failing1().await?;
            } else {
                failing2().await?;
            }
            y.send(Ok("unreachable")).await;
            Ok(())
        })
    }

    let values: Vec<_> = test().collect().await;
    assert_eq!(1, values.len());
    assert_eq!(Err(ErrorB(1)), values[0]);
}

#[tokio::test]
async fn multi_try() {
    fn test() -> impl Stream<Item = Result<i32, String>> {
        generate_try_stream(|mut y| async move {
            let a = Ok::<_, String>(Ok::<_, String>(123))??;
            for _ in 1..10 {
                y.send(Ok(a)).await;
            }
            Ok(())
        })
    }
    let values: Vec<_> = test().collect().await;
    assert_eq!(9, values.len());
    assert_eq!(
        std::iter::repeat(123).take(9).map(Ok).collect::<Vec<_>>(),
        values
    );
}

#[tokio::test]
async fn backpressure() {
    let counter = Rc::new(Cell::new(0u32));

    let counter2 = Rc::clone(&counter);
    let s = generate_stream(|mut y| async move {
        loop {
            let value = counter2.get();
            y.send(value).await;
            counter2.set(value + 1);
        }
    });
    pin_mut!(s);

    for i in 0..200 {
        let counter_value = counter.get();
        assert!(counter_value == i || counter_value + 1 == i);
        let stream_value = s.next().await.unwrap();
        assert_eq!(stream_value, i);
    }
}
