library(plumber)
library(shiny)

#* Echo back the input
#* @get /echo
function(msg = "") {
  list(msg = trimws(msg))
}

#* @post /users/<id:int>
function(id) {
  find_user(id)
}

router <- pr() %>%
  pr_get("/health", function() list(ok = TRUE))

ui <- fluidPage(
  selectInput("region", "Region", choices = c("N", "S")),
  actionButton("refresh", "Refresh"),
  plotOutput("salesPlot")
)

server <- function(input, output, session) {
  sales <- reactive({
    load_sales(input$region)
  })
  observeEvent(input$refresh, {
    showNotification("Refreshed")
  })
  output$salesPlot <- renderPlot({
    plot(sales())
  })
  moduleServer("detail", detail_server)
}

shinyApp(ui = ui, server = server)
