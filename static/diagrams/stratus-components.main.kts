// Dependencies:
// * Kotlin and JVM for running this script.
// * mermaid-cli for Mermaid diagrams.
// * Graphviz for DOT diagrams.
// * PlantUML for PlantUML diagrams.
//
// Usage:
// kotlin stratus-components.main.kts | bash

// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------
@file:DependsOn("com.structurizr:structurizr-core:2.2.0")
@file:DependsOn("com.structurizr:structurizr-export:2.2.0")

import com.structurizr.*
import com.structurizr.export.*
import com.structurizr.export.dot.*
import com.structurizr.export.mermaid.*
import com.structurizr.export.plantuml.*
import com.structurizr.model.*
import com.structurizr.view.*
import java.io.File

// -----------------------------------------------------------------------------
// Setup
// -----------------------------------------------------------------------------
val workspace = Workspace("Stratus", null)
val views = workspace.views
val styles = views.configuration.styles
val model = workspace.model

// -----------------------------------------------------------------------------
// Components
// -----------------------------------------------------------------------------
val stratus = model.addSoftwareSystem("Stratus")
val stratusInternals = stratus.addContainer("Internals")

val importer    = stratusInternals.addComponent("Importer Online")
val rpcServer   = stratusInternals.addComponent("RPC: Server")
val rpcSubs     = stratusInternals.addComponent("RPC: Subscriptions")
val executor    = stratusInternals.addComponent("Core: Executor")
val miner       = stratusInternals.addComponent("Core: Miner")
val storage     = stratusInternals.addComponent("Storage")
val storagePerm = stratusInternals.addComponent("Storage: Permanent").also  { it.technology = "RocksDB";  }
val storageTemp = stratusInternals.addComponent("Storage: Temporary").also { it.technology = "In-Memory"}

// -----------------------------------------------------------------------------
// Relationships
// -----------------------------------------------------------------------------
importer.uses(executor, "execute")

rpcServer.uses(rpcSubs, "manage")
rpcServer.uses(storage, "read")
rpcServer.uses(executor, "execute")

rpcSubs.uses(miner, "subscribe")

executor.uses(storage, "read")
executor.uses(miner, "writes")

miner.uses(storage, "write")

storage.uses(storageTemp, "read/write")
storage.uses(storagePerm, "read/write")

// -----------------------------------------------------------------------------
// Views
// -----------------------------------------------------------------------------
val componentsView = views.createComponentView(stratusInternals, "", "")
componentsView.addAllElements()

// -----------------------------------------------------------------------------
// Exporters
// -----------------------------------------------------------------------------
private fun export(exporter: AbstractDiagramExporter, view: ComponentView): File {
    if (exporter is MermaidDiagramExporter) {
        view.enableAutomaticLayout(AutomaticLayout.RankDirection.LeftRight) // mermaid is bugged
    } else {
        view.enableAutomaticLayout(AutomaticLayout.RankDirection.TopBottom)
    }
    val diagram = exporter.export(componentsView)
    return createTempFile(prefix = "stratus-", suffix = ".diagram").also { it .writeText(diagram.definition) }
}

val mermaidFile = export(MermaidDiagramExporter(), componentsView)
val dotFile = export(DOTExporter(), componentsView)
val plantUmlFile = export(C4PlantUMLExporter(), componentsView)

val renderCommands = listOf(
    "echo Rendering Mermaid",
    "mmdc -i $mermaidFile -o ./rendered-stratus-components-mermaid.svg",
    "echo Rendering Graphviz",
    "dot  -Tsvg $dotFile -o ./rendered-stratus-components-graphviz.svg",
    "echo Rendering PlantUML",
    "java -jar /usr/local/bin/plantuml.jar -failfast -tsvg -o $(pwd)/plantuml $plantUmlFile",
    "mv ./plantuml/*.svg ./rendered-stratus-components-plantuml.svg",
    "rm -rf plantuml"

)
println(renderCommands.joinToString("\n"))

