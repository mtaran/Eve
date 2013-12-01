(ns aurora.service.core
  (:require [compojure.route :as route]
            [compojure.core :refer :all]
            [compojure.handler :as handler]
            [ring.adapter.jetty :as jetty]
            [aurora.service.compiler :as comp]
            ))

(defroutes main-routes
  (GET "/" [] (do (println "hey") (slurp "/users/chris/repos/aurora/index.html")))
  (GET "/bootstrap.js" [] (slurp "/users/chris/repos/aurora/bootstrap.js"))
  (POST "/code" [code ns-prefix]
        {:status 200
         :headers {"Content-Type" "text/javascript; charset=utf-8"
                   "Access-Control-Allow-Origin" "*"}
         :body (try (comp/compile-pipeline code ns-prefix)
                 (catch Exception e
                   (str e)))})
  (route/resources "/resources/" {:root ""})
  (route/not-found "<h1>Page not found</h1>"))

(def app
  (-> (handler/site main-routes)
      ))

(defn -main []
  (jetty/run-jetty (var app) {:port 8082 :join? false}))

(comment

(-main)

  )

