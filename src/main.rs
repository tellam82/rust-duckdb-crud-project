#[macro_use]
extern crate rocket;

use rocket::{Rocket, Build, response::content, serde::json::Json};
use rocket::serde::{Deserialize, Serialize};
use duckdb::{Connection, Result,params};

use rocket::response::content::RawHtml;
use rocket::fs::FileServer;

use duckdb::ToSql;

use std::sync::Mutex;
use rocket::State;

struct DbConn(Mutex<Connection>);

// Define your data structures
#[derive(Deserialize,Serialize)]
#[serde(crate = "rocket::serde")]
struct User {
    id: i32,
    name: String,
    email: String,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct UpdateUser {
    id: i32,
    name: Option<String>,
    email: Option<String>,
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct ApiResponse {
    status: String,
    message: String,
}


// Database setup function
fn setup_database(conn: &Connection) -> Result<()> {
    conn.execute("CREATE TABLE IF NOT EXISTS users1 (id INTEGER PRIMARY KEY, name TEXT, email TEXT UNIQUE)", [])?;
    Ok(())
}

//CRUD operations
#[post("/insert", format = "json", data = "<user>")]
fn insert_user(conn: &State<DbConn>, user: Json<User>) -> Json<ApiResponse> {
    let mut connection = conn.0.lock().expect("db connection lock");

    let transaction = match connection.transaction() {
        Ok(tx) => tx,
        Err(e) => return Json(ApiResponse {
            status: "error".to_string(),
            message: format!("Failed to start transaction: {}", e),
        }),
    };

    let query = "INSERT INTO users1 (id, name, email) VALUES (?1, ?2, ?3)";
    match transaction.execute(query, params![user.id, user.name, user.email]) {
        Ok(_) => {
            transaction.commit().expect("Failed to commit transaction");
            Json(ApiResponse {
                status: "success".to_string(),
                message: "User inserted successfully".to_string(),
            })
        },
        Err(e) => {
            transaction.rollback().expect("Failed to rollback transaction");
            // Specific handling for unique constraint violation
            if e.to_string().contains("UNIQUE constraint failed") {
                Json(ApiResponse {
                    status: "error".to_string(),
                    message: "User already exists".to_string(),
                })
            } else {
                Json(ApiResponse {
                    status: "error".to_string(),
                    message: format!("Failed to insert user: {}", e),
                })
            }
        },
    }
}




#[post("/update", format = "json", data = "<user>")]
fn update_user(conn: &State<DbConn>, user: Json<UpdateUser>) -> Json<ApiResponse> {
    let mut connection = conn.0.lock().expect("db connection lock");
    
    // Start a transaction
    let transaction = connection.transaction().expect("Failed to start transaction");
    let mut query = String::from("UPDATE users1 SET ");
    let mut params: Vec<&dyn ToSql> = vec![];

    if let Some(name) = &user.name {
        query.push_str("name = ?, ");
        params.push(name as &dyn ToSql);
    }
    if let Some(email) = &user.email {
        query.push_str("email = ?, ");
        params.push(email as &dyn ToSql);
    }
    query.pop(); // Remove the last comma
    query.pop(); // Remove the last space
    query.push_str(" WHERE id = ?");
    params.push(&user.id as &dyn ToSql);

    match transaction.execute(&query, &*params) {
        Ok(_) => {
            // Commit the transaction
            transaction.commit().expect("Failed to commit transaction");
            Json(ApiResponse {
                status: "success".to_string(),
                message: "User updated successfully".to_string(),
            })
        },
        Err(e) => {
            // Rollback the transaction in case of error
            transaction.rollback().expect("Failed to rollback transaction");
            Json(ApiResponse {
                status: "error".to_string(),
                message: format!("Failed to update user: {}", e),
            })
        },
    }
}


#[post("/delete", format = "json", data = "<user_id>")]
fn delete_user(conn: &State<DbConn>, user_id: Json<i32>) -> Json<ApiResponse> {
    let mut db_conn = conn.0.lock().expect("db connection lock");
    let transaction = db_conn.transaction().expect("Failed to start transaction");

    //let user_id = user_id.into_inner(); // Convert Json<i32> into i32

    let query = "DELETE FROM users1 WHERE id = ?";
    match transaction.execute(query, &[&user_id.into_inner()]) {
        Ok(_) => {
            transaction.commit().expect("Failed to commit transaction");
            Json(ApiResponse {
                status: "success".to_string(),
                message: "User deleted successfully".to_string(),
            })
        },
        Err(e) => {
            let _ = transaction.rollback(); // Even if rollback fails, we're not handling it here
            Json(ApiResponse {
                status: "error".to_string(),
                message: format!("Failed to delete user: {}", e),
            })
        },
    }
}




//Get the user data
#[get("/user/<user_id>")]
fn get_user(conn: &State<DbConn>, user_id: i32) -> Result<Json<User>, Json<ApiResponse>> {
    // Get a lock on the connection
    let mut connection = conn.0.lock().expect("db connection lock");

    // Prepare and execute the statement within the same scope to avoid locking issues.
    match connection.prepare("SELECT id, name, email FROM users1 WHERE id = ?") {
        Ok(mut stmt) => {
            // Use the statement to query the user
            match stmt.query_map(params![user_id], |row| {
                Ok(User {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    email: row.get(2)?,
                })
            }) {
                Ok(mut user_iter) => {
                    if let Some(result) = user_iter.next() {
                        result.map(Json).map_err(|e| Json(ApiResponse {
                            status: "error".to_string(),
                            message: format!("Failed to retrieve user: {}", e),
                        }))
                    } else {
                        Err(Json(ApiResponse {
                            status: "error".to_string(),
                            message: "User not found".to_string(),
                        }))
                    }
                },
                Err(e) => Err(Json(ApiResponse {
                    status: "error".to_string(),
                    message: format!("Failed to execute query: {}", e),
                })),
            }
        },
        Err(e) => Err(Json(ApiResponse {
            status: "error".to_string(),
            message: format!("Failed to prepare statement: {}", e),
        })),
    }
}



// Route to serve the HTML page
#[get("/")]
fn index() -> content::RawHtml<String> {
    content::RawHtml(std::fs::read_to_string("static/index.html").unwrap())
}

 

 #[launch]
 fn rocket() -> Rocket<Build> {
    let conn = Connection::open("db_file.duckdb").expect("db connection failed");
    setup_database(&conn).expect("database setup failed");

     rocket::build()
         .manage(DbConn(Mutex::new(conn)))
         .mount("/", routes![index, insert_user, update_user, delete_user,get_user])
         .mount("/static", FileServer::from("static"))  // This line serves files from the `static` directory
 }